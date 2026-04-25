//! Single-worker batch processor over the audio_clips queue.
//!
//! Pulls one pending clip at a time, loads its manifest, runs the archive
//! ASR (cold-loaded inside the impl), persists transcripts + audio_segments
//! rows tied to the clip, and marks the clip processed.
//!
//! This module is the orchestration core. The ASR is abstracted via the
//! `ArchiveAsr` trait so tests can drive the pipeline without sherpa.
//! Clustering and speaker assignment land in Plan Task 9; the production
//! sherpa-backed `ArchiveAsr` impl lands alongside the supervisor wiring
//! in Plan Task 12.

use std::path::PathBuf;
use std::sync::Arc;

use sqlx::SqlitePool;
use tracing::info;
use uuid::Uuid;

use crate::domain::types::{AudioClip, ClipManifest};
use crate::repository::{audio_clip, segment, transcript};

#[derive(Debug, Clone)]
pub struct ArchiveTranscript {
    pub segment_id: Uuid,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Trait abstraction over the archive ASR backend. The trait method is
/// synchronous so implementations can own a !Send sherpa recognizer for
/// the duration of one clip's processing without async-trait machinery —
/// callers wrap with `tokio::task::spawn_blocking` (see `process_clip`).
pub trait ArchiveAsr: Send + Sync + 'static {
    fn transcribe_clip(
        &self,
        manifest: &ClipManifest,
        audio_dir: &std::path::Path,
    ) -> anyhow::Result<Vec<ArchiveTranscript>>;
}

/// Process a single claimed clip: load its manifest, run ASR, persist
/// transcripts, mark processed. The caller (`BatchProcessorHandle`) must
/// have already flipped the row to `running` via `claim_next_pending`.
///
/// Failures inside this function should be reported through
/// `audio_clip::mark_failed` by the caller — `process_clip` itself returns
/// the error and lets the supervisor decide retry vs. terminal.
pub async fn process_clip<A: ArchiveAsr>(
    pool: &SqlitePool,
    asr: Arc<A>,
    clip: &AudioClip,
) -> anyhow::Result<()> {
    let manifest = load_manifest(&clip.manifest_path)?;
    let audio_dir = audio_dir_of(&clip.manifest_path);

    if manifest.segments.is_empty() {
        audio_clip::mark_empty(pool, clip.id).await?;
        return Ok(());
    }

    // 1) Persist audio_segments rows tied to this clip. Idempotent so
    //    retried clips don't duplicate rows.
    for seg in &manifest.segments {
        segment::upsert_segment_for_clip(
            pool,
            seg.id,
            clip.session_id,
            clip.id,
            seg.start_ms,
            seg.end_ms,
        )
        .await?;
    }

    // 2) Run ASR in a blocking task — sherpa recognizers are !Send and
    //    own non-trivial state for the duration of decode.
    let manifest_for_asr = manifest.clone();
    let audio_dir_for_asr = audio_dir.clone();
    let asr_clone = asr.clone();
    let transcripts: Vec<ArchiveTranscript> = tokio::task::spawn_blocking(move || {
        asr_clone.transcribe_clip(&manifest_for_asr, &audio_dir_for_asr)
    })
    .await??;

    // 3) Persist transcripts. Each transcript is finalized (the batch ASR
    //    has full audio context — there are no partials).
    for t in &transcripts {
        transcript::create_transcript(
            pool,
            clip.session_id,
            &t.text,
            t.start_ms,
            t.end_ms,
            true, // is_final
            Some(t.segment_id),
        )
        .await?;
    }

    audio_clip::mark_processed(pool, clip.id, None).await?;
    info!(
        clip_id = %clip.id,
        transcripts = transcripts.len(),
        "clip processed"
    );
    Ok(())
}

/// Trait abstraction over the speaker embedder. Mirrors `ArchiveAsr` —
/// implementations own !Send sherpa state, callers wrap with
/// `spawn_blocking`. Returns `(segment_id, embedding)` pairs in the same
/// order as the manifest's segments.
pub trait SegmentEmbedder: Send + Sync + 'static {
    fn embed_segments(
        &self,
        manifest: &ClipManifest,
        audio_dir: &std::path::Path,
    ) -> anyhow::Result<Vec<(Uuid, Vec<f32>)>>;
    fn dimension(&self) -> i64;
}

/// Knobs for the per-clip clustering + speaker-assignment pass. Mirrors a
/// subset of `AudioSettings`; the supervisor (Plan Task 12) wires real
/// settings through.
#[derive(Debug, Clone, Copy)]
pub struct ClusteringConfig {
    pub cosine_threshold: f32,
    pub min_segments_per_cluster: usize,
    pub confirm_threshold: f32,
}

/// Process a clip with the full pipeline: ASR + embedding + clustering +
/// per-cluster speaker matching + auto-provisional creation, then optional
/// post-clip action-item extraction via the LLM router. Each cluster
/// member gets `speaker_id` and `clip_local_speaker_idx` set; clusters
/// below `min_segments_per_cluster` are dropped (segments stay attached
/// to the clip but speaker_id stays NULL).
///
/// The `router` argument is optional so tests can drive the pipeline
/// without an LLM stub. When `None`, the post-clip extractor is skipped.
pub async fn process_clip_with_clustering<A: ArchiveAsr, E: SegmentEmbedder>(
    pool: &SqlitePool,
    asr: Arc<A>,
    embedder: Arc<E>,
    clip: &AudioClip,
    cfg: &ClusteringConfig,
    router: Option<&crate::engine::llm_router::LlmRouter>,
) -> anyhow::Result<()> {
    let manifest = load_manifest(&clip.manifest_path)?;
    let audio_dir = audio_dir_of(&clip.manifest_path);

    if manifest.segments.is_empty() {
        audio_clip::mark_empty(pool, clip.id).await?;
        return Ok(());
    }

    // 1) Persist segment rows tied to the clip.
    for seg in &manifest.segments {
        segment::upsert_segment_for_clip(
            pool,
            seg.id,
            clip.session_id,
            clip.id,
            seg.start_ms,
            seg.end_ms,
        )
        .await?;
    }

    // 2) Embed every segment in a blocking task (sherpa is !Send).
    let manifest_for_emb = manifest.clone();
    let audio_dir_for_emb = audio_dir.clone();
    let embedder_clone = embedder.clone();
    let embeddings: Vec<(Uuid, Vec<f32>)> = tokio::task::spawn_blocking(move || {
        embedder_clone.embed_segments(&manifest_for_emb, &audio_dir_for_emb)
    })
    .await??;
    let dim = embedder.dimension();
    for (id, emb) in &embeddings {
        segment::set_embedding(pool, *id, emb, dim).await?;
    }

    // 3) Run ASR.
    let manifest_for_asr = manifest.clone();
    let audio_dir_for_asr = audio_dir.clone();
    let asr_clone = asr.clone();
    let transcripts: Vec<ArchiveTranscript> = tokio::task::spawn_blocking(move || {
        asr_clone.transcribe_clip(&manifest_for_asr, &audio_dir_for_asr)
    })
    .await??;
    for t in &transcripts {
        transcript::create_transcript(
            pool,
            clip.session_id,
            &t.text,
            t.start_ms,
            t.end_ms,
            true,
            Some(t.segment_id),
        )
        .await?;
    }

    // 4) Cluster.
    let assignments = crate::engine::cluster::ahc(&embeddings, cfg.cosine_threshold);
    // (cluster_idx → Vec<(seg_id, &embedding)>)
    let mut clusters: std::collections::BTreeMap<usize, Vec<(Uuid, &Vec<f32>)>> =
        Default::default();
    for (i, a) in assignments.iter().enumerate() {
        clusters
            .entry(a.cluster_idx)
            .or_default()
            .push((a.segment_id, &embeddings[i].1));
    }

    // 5) For each cluster: centroid → match against speakers → assign or
    //    create provisional → write speaker_id + clip_local_speaker_idx.
    let tenant_id = Uuid::nil();
    for (cluster_idx, members) in clusters {
        if members.len() < cfg.min_segments_per_cluster {
            continue;
        }
        let centroid = mean_unit(members.iter().map(|(_, e)| e.as_slice()));
        let speaker_id = match crate::repository::speaker::find_match_by_centroid(
            pool,
            &centroid,
            dim,
            tenant_id,
            cfg.confirm_threshold,
        )
        .await?
        {
            Some(id) => {
                crate::repository::speaker::touch_provisional_match(pool, id).await?;
                id
            }
            None => {
                let new_id = Uuid::new_v4();
                let now = chrono::Utc::now();
                let display_name = format!("Unknown {}", now.format("%Y-%m-%d %H:%M"));
                crate::repository::speaker::insert_provisional(
                    pool,
                    new_id,
                    tenant_id,
                    &display_name,
                    "#9E9E9E",
                )
                .await?;
                new_id
            }
        };
        for (seg_id, _) in members {
            segment::assign_speaker_and_local_idx(pool, seg_id, speaker_id, cluster_idx as i64)
                .await?;
        }
    }

    audio_clip::mark_processed(pool, clip.id, None).await?;
    info!(
        clip_id = %clip.id,
        clusters = assignments.iter().map(|a| a.cluster_idx).max().map(|m| m + 1).unwrap_or(0),
        transcripts = transcripts.len(),
        "clip processed with clustering"
    );

    // Best-effort post-clip extraction. Errors are already swallowed inside
    // extract_for_clip so the clip stays 'processed' even if the LLM step
    // misbehaves — the user can re-enable an LLM later for future clips.
    if let Some(r) = router {
        let _ = crate::engine::window_extractor::extract_for_clip(pool, r, clip.id).await;
    }
    Ok(())
}

fn mean_unit<'a, I>(vecs: I) -> Vec<f32>
where
    I: IntoIterator<Item = &'a [f32]>,
{
    let mut iter = vecs.into_iter();
    let first = match iter.next() {
        Some(v) => v.to_vec(),
        None => return Vec::new(),
    };
    let dim = first.len();
    let mut sum = first;
    let mut n = 1usize;
    for v in iter {
        for (i, x) in v.iter().enumerate() {
            sum[i] += x;
        }
        n += 1;
    }
    for x in sum.iter_mut() {
        *x /= n as f32;
    }
    let norm = sum.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-8);
    for x in sum.iter_mut() {
        *x /= norm;
    }
    let _ = dim;
    sum
}

fn load_manifest(manifest_path: &str) -> anyhow::Result<ClipManifest> {
    let body = std::fs::read_to_string(manifest_path)?;
    Ok(serde_json::from_str(&body)?)
}

// ── Production async pipeline (sherpa ASR + diarization embed) ───────────

/// Process one clip end-to-end without going through the ArchiveAsr /
/// SegmentEmbedder traits. The traits exist for in-memory tests; this
/// path is what production runs against real sherpa-onnx workers.
///
/// Steps mirror `process_clip_with_clustering` but call engine::asr's
/// offline helpers and engine::diarization::extract_embedding directly
/// — those are async and can't be used from a sync trait method without
/// awkward runtime gymnastics.
pub async fn process_clip_production(
    pool: &SqlitePool,
    clip: &AudioClip,
    archive_model_id: &str,
    embedding_model_path: Option<std::path::PathBuf>,
    model_paths: &crate::engine::model_manager::ModelPaths,
    cluster_cosine_threshold: f32,
    confirm_threshold: f32,
    router: Option<&crate::engine::llm_router::LlmRouter>,
) -> anyhow::Result<()> {
    let manifest = load_manifest(&clip.manifest_path)?;
    let audio_dir = audio_dir_of(&clip.manifest_path);

    if manifest.segments.is_empty() {
        audio_clip::mark_empty(pool, clip.id).await?;
        return Ok(());
    }

    // 1) Persist segment rows tied to the clip.
    for seg in &manifest.segments {
        segment::upsert_segment_for_clip(
            pool,
            seg.id,
            clip.session_id,
            clip.id,
            seg.start_ms,
            seg.end_ms,
        )
        .await?;
    }

    // 2) Run ASR against every segment by feeding them through one of the
    //    engine::asr offline helpers. We collect transcripts indexed by
    //    segment_id so order doesn't matter.
    let (seg_tx, seg_rx) =
        tokio::sync::mpsc::channel::<crate::engine::vad::SpeechSegment>(32);
    let mut transcript_rx = match start_offline_asr_for_archive(
        archive_model_id,
        model_paths,
        seg_rx,
    ) {
        Ok(rx) => rx,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "archive ASR start failed for model {}: {}",
                archive_model_id,
                e
            ));
        }
    };

    // Push every segment WAV into the ASR. We read each WAV synchronously
    // (cheap) but await the send so backpressure works.
    for seg in &manifest.segments {
        let wav_path = audio_dir.join(&seg.file);
        let audio = match read_wav_f32_mono_16k(&wav_path) {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(?wav_path, error=%e, "skipping segment with unreadable WAV");
                continue;
            }
        };
        let speech = crate::engine::vad::SpeechSegment {
            segment_id: seg.id,
            start_sample: ((seg.start_ms as i64 * 16_000) / 1_000) as usize,
            end_sample: ((seg.end_ms as i64 * 16_000) / 1_000) as usize,
            audio,
        };
        if seg_tx.send(speech).await.is_err() {
            tracing::warn!("archive ASR consumer closed early — bailing");
            break;
        }
    }
    drop(seg_tx); // signal end-of-stream so the recognizer drains.

    // Drain transcripts. Stop when the channel closes (recognizer thread
    // exited after seeing the EOF). We add a generous timeout per receive
    // so a stuck worker doesn't hang the queue forever.
    let mut transcripts: Vec<ArchiveTranscript> = Vec::with_capacity(manifest.segments.len());
    let drain_deadline = std::time::Instant::now()
        + std::time::Duration::from_secs((manifest.segments.len() as u64).max(1) * 30);
    loop {
        let remaining = drain_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            tracing::warn!(clip_id=%clip.id, "archive ASR drain timed out");
            break;
        }
        match tokio::time::timeout(remaining, transcript_rx.recv()).await {
            Ok(Some(t)) => {
                if !t.is_final {
                    continue;
                }
                if t.text.trim().is_empty() {
                    continue;
                }
                let segment_id = match t.segment_id {
                    Some(id) => id,
                    None => continue,
                };
                let start_ms = (t.start_sample as i64 * 1_000) / 16_000;
                let end_ms = (t.end_sample as i64 * 1_000) / 16_000;
                transcripts.push(ArchiveTranscript {
                    segment_id,
                    start_ms,
                    end_ms,
                    text: t.text,
                });
            }
            Ok(None) => break,
            Err(_) => {
                tracing::warn!(clip_id=%clip.id, "archive ASR drain timeout — partial transcripts");
                break;
            }
        }
    }

    // Persist transcripts.
    for t in &transcripts {
        transcript::create_transcript(
            pool,
            clip.session_id,
            &t.text,
            t.start_ms,
            t.end_ms,
            true,
            Some(t.segment_id),
        )
        .await?;
    }

    // 3) Run embeddings — only if a speaker-embedding model is configured.
    if let Some(emb_path) = embedding_model_path.as_ref() {
        let mut embeddings: Vec<(Uuid, Vec<f32>)> = Vec::with_capacity(manifest.segments.len());
        let mut dim: i64 = 0;
        for seg in &manifest.segments {
            let wav_path = audio_dir.join(&seg.file);
            let audio = match read_wav_f32_mono_16k(&wav_path) {
                Ok(a) => a,
                Err(_) => continue,
            };
            match crate::engine::diarization::extract_embedding(emb_path, &audio).await {
                Ok(e) => {
                    if dim == 0 {
                        dim = e.values.len() as i64;
                    }
                    if e.values.len() as i64 != dim {
                        // Mid-clip dimension mismatch — skip this segment.
                        continue;
                    }
                    segment::set_embedding(pool, seg.id, &e.values, dim).await?;
                    embeddings.push((seg.id, e.values));
                }
                Err(err) => {
                    tracing::warn!(seg_id=%seg.id, error=%err, "embedding extraction failed");
                }
            }
        }

        // 4) Cluster + 5) Match / insert provisional / assign segments.
        if !embeddings.is_empty() && dim > 0 {
            let assignments =
                crate::engine::cluster::ahc(&embeddings, cluster_cosine_threshold);
            let mut clusters: std::collections::BTreeMap<usize, Vec<(Uuid, &Vec<f32>)>> =
                Default::default();
            for (i, a) in assignments.iter().enumerate() {
                clusters
                    .entry(a.cluster_idx)
                    .or_default()
                    .push((a.segment_id, &embeddings[i].1));
            }
            let tenant_id = Uuid::nil();
            for (cluster_idx, members) in clusters {
                let centroid = mean_unit(members.iter().map(|(_, e)| e.as_slice()));
                let speaker_id = match crate::repository::speaker::find_match_by_centroid(
                    pool,
                    &centroid,
                    dim,
                    tenant_id,
                    confirm_threshold,
                )
                .await?
                {
                    Some(id) => {
                        crate::repository::speaker::touch_provisional_match(pool, id).await?;
                        id
                    }
                    None => {
                        let new_id = Uuid::new_v4();
                        let now = chrono::Utc::now();
                        let display_name = format!("Unknown {}", now.format("%Y-%m-%d %H:%M"));
                        crate::repository::speaker::insert_provisional(
                            pool,
                            new_id,
                            tenant_id,
                            &display_name,
                            "#9E9E9E",
                        )
                        .await?;
                        new_id
                    }
                };
                for (seg_id, _) in members {
                    segment::assign_speaker_and_local_idx(
                        pool,
                        seg_id,
                        speaker_id,
                        cluster_idx as i64,
                    )
                    .await?;
                }
            }
        }
    }

    audio_clip::mark_processed(pool, clip.id, Some(archive_model_id)).await?;
    info!(
        clip_id = %clip.id,
        transcripts = transcripts.len(),
        "clip processed (production)"
    );

    if let Some(r) = router {
        let _ = crate::engine::window_extractor::extract_for_clip(pool, r, clip.id).await;
    }
    Ok(())
}

fn start_offline_asr_for_archive(
    asr_model_id: &str,
    model_paths: &crate::engine::model_manager::ModelPaths,
    seg_rx: tokio::sync::mpsc::Receiver<crate::engine::vad::SpeechSegment>,
) -> anyhow::Result<tokio::sync::mpsc::Receiver<crate::engine::asr::TranscriptResult>> {
    match asr_model_id {
        "whisper_base" => {
            let files = model_paths
                .whisper_base
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Whisper base model not downloaded"))?;
            crate::engine::asr::start_whisper_asr(files, seg_rx)
        }
        "whisper_turbo" => {
            let files = model_paths
                .whisper_turbo
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Whisper turbo model not downloaded"))?;
            crate::engine::asr::start_whisper_asr(files, seg_rx)
        }
        "moonshine_tiny_en" => {
            let files = model_paths
                .moonshine_en
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Moonshine not downloaded"))?;
            crate::engine::asr::start_moonshine_asr(files, seg_rx)
        }
        "paraformer_zh_small" => {
            let files = model_paths
                .paraformer_zh_small
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Paraformer zh-small not downloaded"))?;
            crate::engine::asr::start_paraformer_offline_asr(files, seg_rx)
        }
        "zipformer_ctc_zh_small" => {
            let files = model_paths
                .zipformer_ctc_zh_small
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Zipformer CTC zh-small not downloaded"))?;
            crate::engine::asr::start_zipformer_ctc_asr(files, seg_rx)
        }
        "funasr_nano" => {
            let files = model_paths
                .funasr_nano
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("FunASR Nano not downloaded"))?;
            crate::engine::asr::start_funasr_nano_asr(files, seg_rx)
        }
        _ => {
            let files = model_paths
                .sense_voice
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("SenseVoice not downloaded"))?;
            crate::engine::asr::start_sense_voice_asr(files, seg_rx)
        }
    }
}

fn read_wav_f32_mono_16k(path: &std::path::Path) -> anyhow::Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    anyhow::ensure!(spec.sample_rate == 16_000, "expected 16 kHz wav");
    anyhow::ensure!(spec.channels == 1, "expected mono wav");
    if spec.sample_format == hound::SampleFormat::Float {
        Ok(reader.samples::<f32>().filter_map(|s| s.ok()).collect())
    } else {
        let max = (1i32 << (spec.bits_per_sample - 1)) as f32;
        Ok(reader
            .samples::<i32>()
            .filter_map(|s| s.ok())
            .map(|x| x as f32 / max)
            .collect())
    }
}

/// ProductionClipRunner — the ClipRunner the supervisor wires into the
/// BatchProcessorHandle. Reads settings on every clip so model selection
/// + thresholds can change without restarting the loop.
pub struct ProductionClipRunner {
    pub settings_manager: Arc<crate::engine::app_settings::SettingsManager>,
    pub model_manager: Arc<crate::engine::model_manager::ModelManager>,
    pub router: Arc<tokio::sync::RwLock<crate::engine::llm_router::LlmRouter>>,
}

impl ClipRunner for ProductionClipRunner {
    fn run_clip<'a>(
        &'a self,
        pool: &'a sqlx::SqlitePool,
        clip: &'a AudioClip,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>>
    {
        Box::pin(async move {
            let settings = self.settings_manager.get().await;
            let resolved = settings.audio.resolved_asr_models();
            let archive_model = resolved
                .archive
                .ok_or_else(|| anyhow::anyhow!("no archive ASR model configured"))?;
            let embedding_id = settings.audio.speaker_embedding_model.clone();
            let model_paths = self
                .model_manager
                .model_paths(embedding_id.as_deref())
                .await
                .ok_or_else(|| anyhow::anyhow!("models not ready for archive processing"))?;

            let router = self.router.read().await;
            let router_ref = if router.is_disabled() {
                None
            } else {
                Some(&*router)
            };
            process_clip_production(
                pool,
                clip,
                &archive_model,
                model_paths.speaker_embedding.clone(),
                &model_paths,
                settings.audio.cluster_cosine_threshold,
                settings.audio.speaker_confirm_threshold,
                router_ref,
            )
            .await
        })
    }
}

// ── BatchProcessor lifecycle handle ──────────────────────────────────────

/// Long-lived handle that owns the claim → process → mark loop. The
/// supervisor calls `ensure_running` when `always_listening = true` and
/// `ensure_stopped` when the user toggles to privacy mode. The loop
/// itself is single-worker process-wide, matching the spec's intent that
/// only one clip at a time gets the cold-loaded archive ASR.
pub struct BatchProcessorHandle {
    inner: tokio::sync::Mutex<HandleInner>,
}

struct HandleInner {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

impl BatchProcessorHandle {
    pub fn new() -> Self {
        Self {
            inner: tokio::sync::Mutex::new(HandleInner { cancel: None }),
        }
    }

    pub async fn is_running(&self) -> bool {
        self.inner.lock().await.cancel.is_some()
    }

    /// Start the loop if it isn't already. Runner is a closure that
    /// processes a single claimed clip — wired in by the supervisor with
    /// the pool, ASR, and embedder it constructs from settings. Decoupled
    /// this way so the handle module doesn't take a dependency on every
    /// concrete sherpa type.
    pub async fn ensure_running<R>(&self, pool: sqlx::SqlitePool, runner: Arc<R>)
    where
        R: ClipRunner + 'static,
    {
        let mut g = self.inner.lock().await;
        if g.cancel.is_some() {
            return;
        }
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        g.cancel = Some(cancel_tx);

        // Boot housekeeping: revert orphan 'running' rows from a prior crash.
        if let Err(e) = audio_clip::requeue_stale_running(&pool).await {
            tracing::warn!(error=%e, "BatchProcessor: requeue stale on boot failed");
        }

        tokio::spawn(async move {
            loop {
                if cancel_rx.try_recv().is_ok() {
                    break;
                }
                match audio_clip::claim_next_pending(&pool).await {
                    Ok(Some(clip)) => match runner.run_clip(&pool, &clip).await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::warn!(error=%e, clip_id=%clip.id, "BatchProcessor: clip failed");
                            let _ = audio_clip::mark_failed(&pool, clip.id, &e.to_string()).await;
                        }
                    },
                    Ok(None) => {
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        tracing::warn!(error=%e, "BatchProcessor: claim failed");
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    }
                }
            }
            tracing::info!("BatchProcessor stopped");
        });
    }

    pub async fn ensure_stopped(&self) {
        let mut g = self.inner.lock().await;
        if let Some(tx) = g.cancel.take() {
            let _ = tx.send(());
        }
    }
}

impl Default for BatchProcessorHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Strategy interface the supervisor wires to the handle. One impl pulls
/// the production sherpa-onnx ASR + embedder; tests inject a stub. Using
/// a boxed-future shape rather than `async fn in trait` keeps the trait
/// dyn-safe so the supervisor can erase the concrete runner type.
pub trait ClipRunner: Send + Sync {
    fn run_clip<'a>(
        &'a self,
        pool: &'a sqlx::SqlitePool,
        clip: &'a AudioClip,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>>;
}

#[cfg(test)]
mod handle_tests {
    use super::*;

    struct CountingRunner {
        count: std::sync::atomic::AtomicUsize,
    }

    impl ClipRunner for CountingRunner {
        fn run_clip<'a>(
            &'a self,
            pool: &'a sqlx::SqlitePool,
            clip: &'a AudioClip,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + 'a>>
        {
            Box::pin(async move {
                self.count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                audio_clip::mark_processed(pool, clip.id, None).await?;
                Ok(())
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_drains_pending_clips_then_idles() {
        let pool = super::tests::fresh_pool().await;
        let session_id = super::tests::mk_session(&pool).await;
        let _ = audio_clip::insert_pending(&pool, session_id, 0, 100, 1, "/tmp/a")
            .await
            .unwrap();
        let _ = audio_clip::insert_pending(&pool, session_id, 100, 200, 1, "/tmp/b")
            .await
            .unwrap();

        let handle = BatchProcessorHandle::new();
        let runner = Arc::new(CountingRunner {
            count: std::sync::atomic::AtomicUsize::new(0),
        });
        handle.ensure_running(pool.clone(), runner.clone()).await;

        // Give the loop a moment to drain. Two clips at idle-poll cadence
        // should be claimed near-instantly.
        for _ in 0..20 {
            if runner.count.load(std::sync::atomic::Ordering::Relaxed) == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(runner.count.load(std::sync::atomic::Ordering::Relaxed), 2);

        handle.ensure_stopped().await;
        assert!(!handle.is_running().await);
    }
}

fn audio_dir_of(manifest_path: &str) -> PathBuf {
    std::path::Path::new(manifest_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::ClipManifestSegment;
    use crate::engine::clip_writer::write_manifest;
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;
    use tempfile::tempdir;

    pub(super) async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    pub(super) async fn mk_session(pool: &SqlitePool) -> Uuid {
        let sid = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO audio_sessions (id, tenant_id, source_type, mode, routing_policy)
               VALUES (?1, '00000000-0000-0000-0000-000000000000', 'microphone', 'realtime', 'default')"#,
        )
        .bind(sid.to_string())
        .execute(pool)
        .await
        .unwrap();
        sid
    }

    struct StubAsr;
    impl ArchiveAsr for StubAsr {
        fn transcribe_clip(
            &self,
            manifest: &ClipManifest,
            _audio_dir: &std::path::Path,
        ) -> anyhow::Result<Vec<ArchiveTranscript>> {
            Ok(manifest
                .segments
                .iter()
                .map(|s| ArchiveTranscript {
                    segment_id: s.id,
                    start_ms: s.start_ms,
                    end_ms: s.end_ms,
                    text: format!("seg-{}", s.start_ms),
                })
                .collect())
        }
    }

    #[tokio::test]
    async fn process_clip_writes_segments_and_transcripts_and_marks_processed() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        let tmp = tempdir().unwrap();
        let manifest = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![
                ClipManifestSegment {
                    id: Uuid::new_v4(),
                    start_ms: 1_000,
                    end_ms: 3_000,
                    file: "seg_0001.wav".into(),
                },
                ClipManifestSegment {
                    id: Uuid::new_v4(),
                    start_ms: 4_000,
                    end_ms: 6_000,
                    file: "seg_0002.wav".into(),
                },
            ],
        };
        let manifest_path = write_manifest(tmp.path(), &manifest).unwrap();
        let clip_id = audio_clip::insert_pending(
            &pool,
            session_id,
            0,
            300_000,
            2,
            manifest_path.to_string_lossy().as_ref(),
        )
        .await
        .unwrap();

        let claimed = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(claimed.id, clip_id);

        process_clip(&pool, Arc::new(StubAsr), &claimed).await.unwrap();

        let after = audio_clip::get_by_id(&pool, clip_id).await.unwrap().unwrap();
        assert_eq!(after.status, "processed");

        let transcripts = transcript::get_final_transcripts_for_session(&pool, session_id)
            .await
            .unwrap();
        assert_eq!(transcripts.len(), 2);
        assert!(transcripts.iter().all(|t| t.text.starts_with("seg-")));
    }

    #[tokio::test]
    async fn process_clip_marks_empty_when_manifest_has_no_segments() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        let tmp = tempdir().unwrap();
        let manifest = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![],
        };
        let manifest_path = write_manifest(tmp.path(), &manifest).unwrap();
        let clip_id = audio_clip::insert_pending(
            &pool,
            session_id,
            0,
            300_000,
            0,
            manifest_path.to_string_lossy().as_ref(),
        )
        .await
        .unwrap();
        let claimed = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();

        process_clip(&pool, Arc::new(StubAsr), &claimed).await.unwrap();

        let after = audio_clip::get_by_id(&pool, clip_id).await.unwrap().unwrap();
        assert_eq!(after.status, "empty");
        let transcripts = transcript::get_final_transcripts_for_session(&pool, session_id)
            .await
            .unwrap();
        assert!(transcripts.is_empty());
    }

    struct StubEmbedder {
        vecs: Vec<Vec<f32>>,
        dim: i64,
    }
    impl SegmentEmbedder for StubEmbedder {
        fn embed_segments(
            &self,
            manifest: &ClipManifest,
            _audio_dir: &std::path::Path,
        ) -> anyhow::Result<Vec<(Uuid, Vec<f32>)>> {
            Ok(manifest
                .segments
                .iter()
                .enumerate()
                .map(|(i, s)| (s.id, self.vecs[i].clone()))
                .collect())
        }
        fn dimension(&self) -> i64 {
            self.dim
        }
    }

    fn cluster_cfg() -> ClusteringConfig {
        ClusteringConfig {
            cosine_threshold: 0.4,
            min_segments_per_cluster: 1,
            confirm_threshold: 0.55,
        }
    }

    #[tokio::test]
    async fn cluster_and_provisional_speakers_get_persisted() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        let tmp = tempdir().unwrap();
        let manifest = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![
                ClipManifestSegment {
                    id: Uuid::new_v4(),
                    start_ms: 1_000,
                    end_ms: 3_000,
                    file: "1.wav".into(),
                },
                ClipManifestSegment {
                    id: Uuid::new_v4(),
                    start_ms: 4_000,
                    end_ms: 6_000,
                    file: "2.wav".into(),
                },
                ClipManifestSegment {
                    id: Uuid::new_v4(),
                    start_ms: 7_000,
                    end_ms: 9_000,
                    file: "3.wav".into(),
                },
            ],
        };
        let manifest_path = write_manifest(tmp.path(), &manifest).unwrap();
        audio_clip::insert_pending(
            &pool,
            session_id,
            0,
            300_000,
            3,
            manifest_path.to_string_lossy().as_ref(),
        )
        .await
        .unwrap();
        let claimed = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();

        // Two collinear vectors (one cluster) + one orthogonal (another cluster).
        let embedder = Arc::new(StubEmbedder {
            vecs: vec![
                vec![1.0, 0.0],
                vec![0.99, 0.14],
                vec![0.0, 1.0],
            ],
            dim: 2,
        });

        process_clip_with_clustering(&pool, Arc::new(StubAsr), embedder, &claimed, &cluster_cfg(), None)
            .await
            .unwrap();

        let provisional =
            crate::repository::speaker::list_provisional(&pool).await.unwrap();
        assert_eq!(provisional.len(), 2, "expected two provisional speakers");

        let segs = crate::repository::segment::list_for_clip(&pool, claimed.id).await.unwrap();
        assert_eq!(segs.len(), 3);
        // First two segments share a cluster index; third differs.
        assert_eq!(segs[0].clip_local_speaker_idx, segs[1].clip_local_speaker_idx);
        assert_ne!(segs[0].clip_local_speaker_idx, segs[2].clip_local_speaker_idx);
        assert!(segs.iter().all(|s| s.speaker_id.is_some()));
        assert_eq!(segs[0].speaker_id, segs[1].speaker_id);
        assert_ne!(segs[0].speaker_id, segs[2].speaker_id);

        let after = audio_clip::get_by_id(&pool, claimed.id).await.unwrap().unwrap();
        assert_eq!(after.status, "processed");
    }

    #[tokio::test]
    async fn second_clip_with_same_centroid_links_to_first_provisional() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let cfg = cluster_cfg();

        // Clip 1: single segment, embedding [1.0, 0.0].
        let tmp = tempdir().unwrap();
        let m1 = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![ClipManifestSegment {
                id: Uuid::new_v4(),
                start_ms: 0,
                end_ms: 1_000,
                file: "a.wav".into(),
            }],
        };
        let p1 = write_manifest(tmp.path(), &m1).unwrap();
        audio_clip::insert_pending(&pool, session_id, 0, 300_000, 1, p1.to_string_lossy().as_ref())
            .await
            .unwrap();
        let c1 = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
        let e1 = Arc::new(StubEmbedder { vecs: vec![vec![1.0, 0.0]], dim: 2 });
        process_clip_with_clustering(&pool, Arc::new(StubAsr), e1, &c1, &cfg, None).await.unwrap();

        // Clip 2: single segment, near-collinear embedding [0.99, 0.14].
        let tmp2 = tempdir().unwrap();
        let m2 = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 300_000,
            ended_at_ms: 600_000,
            segments: vec![ClipManifestSegment {
                id: Uuid::new_v4(),
                start_ms: 300_000,
                end_ms: 301_000,
                file: "b.wav".into(),
            }],
        };
        let p2 = write_manifest(tmp2.path(), &m2).unwrap();
        audio_clip::insert_pending(
            &pool,
            session_id,
            300_000,
            600_000,
            1,
            p2.to_string_lossy().as_ref(),
        )
        .await
        .unwrap();
        let c2 = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
        let e2 = Arc::new(StubEmbedder { vecs: vec![vec![0.99, 0.14]], dim: 2 });
        process_clip_with_clustering(&pool, Arc::new(StubAsr), e2, &c2, &cfg, None).await.unwrap();

        let provisional =
            crate::repository::speaker::list_provisional(&pool).await.unwrap();
        assert_eq!(
            provisional.len(),
            1,
            "second clip should reuse the first's provisional row"
        );

        let segs1 = crate::repository::segment::list_for_clip(&pool, c1.id).await.unwrap();
        let segs2 = crate::repository::segment::list_for_clip(&pool, c2.id).await.unwrap();
        assert_eq!(segs1[0].speaker_id, segs2[0].speaker_id);
    }

    #[tokio::test]
    async fn process_clip_propagates_asr_errors() {
        struct FailingAsr;
        impl ArchiveAsr for FailingAsr {
            fn transcribe_clip(
                &self,
                _manifest: &ClipManifest,
                _audio_dir: &std::path::Path,
            ) -> anyhow::Result<Vec<ArchiveTranscript>> {
                Err(anyhow::anyhow!("model not loaded"))
            }
        }

        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        let tmp = tempdir().unwrap();
        let manifest = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![ClipManifestSegment {
                id: Uuid::new_v4(),
                start_ms: 0,
                end_ms: 1_000,
                file: "s.wav".into(),
            }],
        };
        let manifest_path = write_manifest(tmp.path(), &manifest).unwrap();
        audio_clip::insert_pending(
            &pool,
            session_id,
            0,
            300_000,
            1,
            manifest_path.to_string_lossy().as_ref(),
        )
        .await
        .unwrap();
        let claimed = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();

        let err = process_clip(&pool, Arc::new(FailingAsr), &claimed).await;
        assert!(err.is_err());
        let after = audio_clip::get_by_id(&pool, claimed.id).await.unwrap().unwrap();
        // process_clip itself does not flip status — that's mark_failed's
        // job at the caller layer. We just confirm we didn't accidentally
        // mark it processed on a failed run.
        assert_ne!(after.status, "processed");
    }
}
