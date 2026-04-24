use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine::asr;
use crate::engine::audio_capture::{self, AudioCaptureHandle};
use crate::engine::continuity::{
    self, AttributionOutcome, ContinuityConfig, ContinuityState, MatchEvidence,
};
use crate::engine::live_enrollment::LiveEnrollment;
use crate::engine::model_manager::ModelPaths;
use crate::engine::transcript_aggregator::TranscriptAggregator;
use crate::engine::vad::{self, SpeechSegment, VadConfig};

/// Tunable knobs for the per-segment speaker identifier. Values live in
/// AudioSettings so the user can adjust them without rebuilding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerIdConfig {
    pub confirm_threshold: f32,
    pub tentative_threshold: f32,
    pub min_duration_ms: u32,
    pub continuity_window_ms: u32,
}

impl Default for SpeakerIdConfig {
    fn default() -> Self {
        Self {
            confirm_threshold: 0.55,
            tentative_threshold: 0.40,
            min_duration_ms: 1500,
            continuity_window_ms: 15_000,
        }
    }
}

pub struct InferencePipeline {
    /// The capture handle — dropping it stops capture
    capture_handle: Option<AudioCaptureHandle>,
    /// Cancel token for the pipeline tasks
    cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

// SAFETY: InferencePipeline is always accessed behind a Mutex, so it will never
// be sent across threads concurrently. cpal::Stream contains *mut () which is
// !Send by default, but the stream is created and destroyed on the same thread
// via the Mutex guard.
unsafe impl Send for InferencePipeline {}

impl InferencePipeline {
    pub fn new() -> Self {
        Self {
            capture_handle: None,
            cancel_tx: None,
        }
    }

    /// Start the pipeline for a session. Returns error if models aren't ready.
    ///
    /// `clips_dir` is where Phase-A voiceprint-candidate clips are written.
    /// Segments that fail speaker identification but pass a quality gate
    /// have their raw audio retained here so the user can later be prompted
    /// to identify who was speaking.
    pub fn start_session(
        &mut self,
        session_id: Uuid,
        tenant_id: Uuid,
        model_paths: &ModelPaths,
        aggregator: Arc<TranscriptAggregator>,
        device_name: Option<&str>,
        asr_model: Option<&str>,
        _asr_language: Option<&str>,
        clips_dir: PathBuf,
        live_enrollment: LiveEnrollment,
        speaker_id_config: SpeakerIdConfig,
    ) -> anyhow::Result<()> {
        // 1. Start audio capture
        let (capture_handle, audio_rx) = audio_capture::start_capture(device_name)?;

        // Tap audio chunks to publish smoothed RMS into the live-enrollment
        // state, so the enrollment UI can render a real mic-level meter.
        // Cheap no-op when no enrollment is armed.
        let audio_rx = install_level_observer(audio_rx, live_enrollment.clone());

        // 2. Select ASR model based on user setting and route audio to the
        //    appropriate recognizer.
        //    - Zipformer transducers (zh/en): direct streaming ASR, no VAD.
        //    - SenseVoice: VAD segments fed into offline ASR.
        //    - Other catalog entries are not yet wired.
        let chosen = asr_model.unwrap_or("auto");
        info!(model = chosen, "Selected ASR model");

        match &model_paths.speaker_embedding {
            Some(p) => info!(path = %p.display(), "Speaker embedding model loaded"),
            None => info!("Speaker embedding model not loaded — segments will stay [UNKNOWN]"),
        }

        // Session-scoped state: a fresh Arc each call. Dropping this
        // handle on the next start_session isolates stale in-flight tasks
        // from the new session's state automatically.
        let continuity: Arc<Mutex<ContinuityState>> =
            Arc::new(Mutex::new(ContinuityState::default()));

        // Helper: start VAD, then fan each speech segment into (a) a per-segment
        // speaker-identification task and (b) the downstream offline ASR
        // consumer. Streaming ASR paths use the parallel-VAD variant below
        // so they still get speaker attribution.
        let embedding_model = model_paths.speaker_embedding.clone();
        let pool = aggregator.pool();
        let aggregator_for_hooks = aggregator.clone();
        let start_vad_pipeline = |audio_rx: mpsc::Receiver<Vec<f32>>| -> anyhow::Result<_> {
            if !model_paths.silero_vad.exists() {
                return Err(anyhow::anyhow!(
                    "Silero VAD not found — required for offline models"
                ));
            }
            let upstream = vad::start_vad(&model_paths.silero_vad, VadConfig::default(), audio_rx)?;
            Ok(split_segments_for_speaker_id(
                upstream,
                session_id,
                tenant_id,
                pool.clone(),
                embedding_model.clone(),
                clips_dir.clone(),
                live_enrollment.clone(),
                aggregator_for_hooks.clone(),
                speaker_id_config,
                continuity.clone(),
            ))
        };

        // Helper for streaming ASR: tee the audio stream. Half feeds the
        // streaming recognizer as before; the other half feeds a VAD chain
        // whose sole job is to emit segments for the speaker-id task, so
        // streaming-mode sessions still get speaker_resolved events.
        let start_parallel_speaker_vad =
            |audio_rx: mpsc::Receiver<Vec<f32>>| -> anyhow::Result<()> {
                if !model_paths.silero_vad.exists() {
                    warn!("Silero VAD missing — streaming mode will have no speaker attribution");
                    return Ok(());
                }
                let upstream =
                    vad::start_vad(&model_paths.silero_vad, VadConfig::default(), audio_rx)?;
                spawn_speaker_id_only(
                    upstream,
                    session_id,
                    tenant_id,
                    pool.clone(),
                    embedding_model.clone(),
                    clips_dir.clone(),
                    live_enrollment.clone(),
                    aggregator_for_hooks.clone(),
                    speaker_id_config,
                    continuity.clone(),
                );
                Ok(())
            };

        let transcript_rx = if let Some(files) = model_paths.transducers.get(chosen) {
            // ── Streaming transducer (any model in the table) ────────
            let (audio_for_asr, audio_for_vad) = tee_audio(audio_rx);
            start_parallel_speaker_vad(audio_for_vad)?;
            asr::start_streaming_asr(files, audio_for_asr)?
        } else {
            match chosen {
                // ── Offline models (individual routing) ──────────────
                "whisper_base" => {
                    let files = model_paths
                        .whisper_base
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Whisper base model not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_whisper_asr(files, seg_rx)?
                }
                "whisper_turbo" => {
                    let files = model_paths
                        .whisper_turbo
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Whisper turbo model not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_whisper_asr(files, seg_rx)?
                }
                "zipformer_ctc_zh_small" => {
                    let files = model_paths
                        .zipformer_ctc_zh_small
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Zipformer CTC zh-small not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_zipformer_ctc_asr(files, seg_rx)?
                }
                "paraformer_zh_small" => {
                    let files = model_paths
                        .paraformer_zh_small
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Paraformer zh-small not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_paraformer_offline_asr(files, seg_rx)?
                }
                "sense_voice_multi" => {
                    let files = model_paths
                        .sense_voice
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("SenseVoice not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_sense_voice_asr(files, seg_rx)?
                }
                "moonshine_tiny_en" => {
                    let files = model_paths
                        .moonshine_en
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Moonshine not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_moonshine_asr(files, seg_rx)?
                }
                "funasr_nano" => {
                    let files = model_paths
                        .funasr_nano
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("FunASR Nano not downloaded"))?;
                    let seg_rx = start_vad_pipeline(audio_rx)?;
                    asr::start_funasr_nano_asr(files, seg_rx)?
                }
                // ── Fallback: first available streaming transducer ───
                _ => {
                    let files = model_paths.transducers.values().next().ok_or_else(|| {
                        anyhow::anyhow!("No streaming transducer model available for '{}'", chosen)
                    })?;
                    let (audio_for_asr, audio_for_vad) = tee_audio(audio_rx);
                    start_parallel_speaker_vad(audio_for_vad)?;
                    asr::start_streaming_asr(files, audio_for_asr)?
                }
            }
        };

        // 4. Spawn task to persist transcripts
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let mut transcript_rx = transcript_rx;
            loop {
                tokio::select! {
                    result = transcript_rx.recv() => {
                        match result {
                            Some(t) => {
                                if t.text.trim().is_empty() {
                                    continue;
                                }
                                let start_ms = (t.start_sample as i64 * 1000) / 16000;
                                let end_ms = (t.end_sample as i64 * 1000) / 16000;

                                if t.is_final {
                                    if let Err(e) = aggregator.add_final(
                                        session_id, &t.text, start_ms, end_ms, None,
                                    ).await {
                                        warn!(%session_id, error = %e, "Failed to persist final transcript");
                                    }
                                } else {
                                    // Partials: broadcast to WS only, no DB persist
                                    aggregator.broadcast_partial(&t.text, start_ms, end_ms);
                                }
                            }
                            None => break, // ASR channel closed
                        }
                    }
                    _ = &mut cancel_rx => {
                        info!(%session_id, "Pipeline cancelled");
                        break;
                    }
                }
            }
            info!(%session_id, "Transcript persistence task ended");
        });

        self.capture_handle = Some(capture_handle);
        self.cancel_tx = Some(cancel_tx);

        info!(%session_id, "Inference pipeline started");
        Ok(())
    }

    /// Stop the pipeline (called on session end)
    pub fn stop(&mut self) {
        if let Some(cancel) = self.cancel_tx.take() {
            let _ = cancel.send(());
        }
        self.capture_handle.take(); // Drop stops the stream
    }

    pub fn is_running(&self) -> bool {
        self.capture_handle.is_some()
    }
}

impl Default for InferencePipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InferencePipeline {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Interpose between VAD output and the downstream (offline-ASR) consumer.
/// Each `SpeechSegment` is forwarded FIRST to the downstream ASR consumer so
/// transcription is not delayed by the embedding + identify hop. Then
/// `run_segment_hook` is awaited inline so the continuity state machine sees
/// segments in strict VAD emission order.
#[allow(clippy::too_many_arguments)]
fn split_segments_for_speaker_id(
    mut upstream: mpsc::Receiver<SpeechSegment>,
    session_id: Uuid,
    tenant_id: Uuid,
    pool: sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    clips_dir: PathBuf,
    live_enrollment: LiveEnrollment,
    aggregator: Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: Arc<Mutex<ContinuityState>>,
) -> mpsc::Receiver<SpeechSegment> {
    let (tx, rx) = mpsc::channel::<SpeechSegment>(32);
    tokio::spawn(async move {
        while let Some(seg) = upstream.recv().await {
            // 1. Forward to downstream offline ASR FIRST so transcription
            //    is not delayed by the embedding + identify hop.
            if tx.send(seg.clone()).await.is_err() {
                break; // downstream ASR consumer went away
            }
            // 2. Run the speaker-id hook inline. The next recv() does not
            //    fire until this returns, so the state machine sees
            //    segments strictly in VAD emission order.
            run_segment_hook(
                seg,
                session_id,
                tenant_id,
                pool.clone(),
                embedding_model.clone(),
                clips_dir.clone(),
                live_enrollment.clone(),
                aggregator.clone(),
                speaker_id_config,
                continuity.clone(),
            )
            .await;
        }
    });
    rx
}

/// Streaming-mode variant: runs speaker-id over VAD segments but does NOT
/// forward them anywhere (there's no downstream ASR — the streaming
/// recognizer is consuming the raw audio in parallel).
#[allow(clippy::too_many_arguments)]
fn spawn_speaker_id_only(
    mut upstream: mpsc::Receiver<SpeechSegment>,
    session_id: Uuid,
    tenant_id: Uuid,
    pool: sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    clips_dir: PathBuf,
    live_enrollment: LiveEnrollment,
    aggregator: Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: Arc<Mutex<ContinuityState>>,
) {
    tokio::spawn(async move {
        while let Some(seg) = upstream.recv().await {
            run_segment_hook(
                seg,
                session_id,
                tenant_id,
                pool.clone(),
                embedding_model.clone(),
                clips_dir.clone(),
                live_enrollment.clone(),
                aggregator.clone(),
                speaker_id_config,
                continuity.clone(),
            )
            .await;
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_segment_hook(
    seg: SpeechSegment,
    session_id: Uuid,
    tenant_id: Uuid,
    pool: sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    clips_dir: PathBuf,
    live_enrollment: LiveEnrollment,
    aggregator: Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: Arc<Mutex<ContinuityState>>,
) {
    let start_ms = (seg.start_sample as i64 * 1000) / 16000;
    let end_ms = (seg.end_sample as i64 * 1000) / 16000;
    let segment_id = seg.segment_id;
    let audio = seg.audio;

    if let Err(e) = handle_segment_embedding(
        &pool,
        embedding_model,
        session_id,
        tenant_id,
        segment_id,
        start_ms,
        end_ms,
        audio,
        &clips_dir,
        &live_enrollment,
        &aggregator,
        speaker_id_config,
        &continuity,
    )
    .await
    {
        warn!(%session_id, error = %e, "segment speaker-id hook failed");
    }
}

/// Interpose between the capture source and the rest of the pipeline to
/// compute a smoothed RMS per chunk and publish it into the enrollment state.
/// Kept always-on (cheap few-hundred-flops per chunk) so the meter is already
/// reflecting current audio the moment the user hits "record voiceprint".
fn install_level_observer(
    mut rx: mpsc::Receiver<Vec<f32>>,
    live_enrollment: LiveEnrollment,
) -> mpsc::Receiver<Vec<f32>> {
    let (tx, out_rx) = mpsc::channel::<Vec<f32>>(64);
    tokio::spawn(async move {
        // Exponential moving average — a single loud sample shouldn't peg
        // the meter. ~20ms chunks × 0.3 alpha gives roughly a 70ms response.
        let mut ema = 0.0f32;
        while let Some(chunk) = rx.recv().await {
            if !chunk.is_empty() {
                let sum_sq: f32 = chunk.iter().map(|x| x * x).sum();
                let rms = (sum_sq / chunk.len() as f32).sqrt();
                ema = 0.7 * ema + 0.3 * rms;
                crate::engine::live_enrollment::publish_level(&live_enrollment, ema);
            }
            if tx.send(chunk).await.is_err() {
                break;
            }
        }
    });
    out_rx
}

/// Tee a single audio stream into two independent receivers. Used when the
/// streaming recognizer needs the raw samples AND a parallel VAD chain
/// needs them for speaker identification.
fn tee_audio(
    mut rx: mpsc::Receiver<Vec<f32>>,
) -> (mpsc::Receiver<Vec<f32>>, mpsc::Receiver<Vec<f32>>) {
    let (tx_a, rx_a) = mpsc::channel::<Vec<f32>>(64);
    let (tx_b, rx_b) = mpsc::channel::<Vec<f32>>(64);
    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            // Best-effort send to both consumers. If one has disconnected,
            // keep feeding the other so the surviving path stays live.
            let _ = tx_a.send(chunk.clone()).await;
            let _ = tx_b.send(chunk).await;
        }
    });
    (rx_a, rx_b)
}

/// Minimum quality score for an unknown segment to be retained as a
/// voiceprint candidate. Empirically tuned: rms in the 0.05..0.30 band
/// combined with at least ~4.5s of duration produces a score ≥ 0.6 through
/// `audio_quality::score`.
const VOICEPRINT_CANDIDATE_QUALITY: f32 = 0.6;

/// Shared tail for every path that wants to participate in continuity:
/// too-short, below-threshold, Tentative, and Confirmed. Runs the state
/// machine, persists the segment row with the outcome's speaker_id, and
/// publishes the corresponding WS event.
///
/// `match_similarity` is `Some` only for paths whose `evidence` came from
/// `identify_speaker_with_thresholds` and where `carried_over == false`.
/// Carry-over / too-short / matcher-error paths persist a NULL
/// `speaker_score` so downstream consumers don't see a fabricated value.
#[allow(clippy::too_many_arguments)]
async fn finalize_segment(
    pool: &sqlx::SqlitePool,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    embedding: Option<&[f32]>,
    audio: &[f32],
    audio_quality: f32,
    clips_dir: &Path,
    evidence: MatchEvidence,
    match_similarity: Option<f64>,
    continuity: &Arc<Mutex<ContinuityState>>,
    config: ContinuityConfig,
    aggregator: &Arc<TranscriptAggregator>,
    segment_id: Uuid,
) -> anyhow::Result<Option<String>> {
    // Run the state machine under a short-lived lock.
    let outcome: AttributionOutcome = {
        let mut guard = continuity.lock().await;
        let (outcome, new_state) = continuity::next_attribution(&*guard, end_ms, evidence, config);
        *guard = new_state;
        outcome
    };

    let persisted_score: Option<f64> = if outcome.carried_over {
        None
    } else {
        match_similarity
    };

    // Candidate-clip retention keys off the FINAL outcome, not the raw
    // matcher result. Carry-over turns an unknown into an attributed
    // segment, so we should not retain its audio as a Phase-A candidate.
    let audio_ref: Option<String> =
        if outcome.speaker_id.is_none() && audio_quality >= VOICEPRINT_CANDIDATE_QUALITY {
            let candidate_id = Uuid::new_v4().to_string();
            match crate::engine::clip_storage::write_clip(clips_dir, &candidate_id, audio) {
                Ok(name) => Some(name),
                Err(err) => {
                    warn!(
                        ?err,
                        ?clips_dir,
                        "failed to retain voiceprint-candidate clip"
                    );
                    None
                }
            }
        } else {
            None
        };

    crate::repository::segment::insert_segment(
        pool,
        session_id,
        start_ms,
        end_ms,
        outcome.speaker_id,
        persisted_score,
        embedding,
        audio_ref.as_deref(),
    )
    .await?;

    aggregator.publish_speaker_resolved(
        crate::engine::transcript_aggregator::SpeakerResolvedEvent {
            segment_id: segment_id.to_string(),
            start_ms,
            end_ms,
            speaker_id: outcome.speaker_id.map(|u| u.to_string()),
            confidence: outcome.confidence.map(|c| c.as_str()),
            carried_over: outcome.carried_over,
        },
    );

    Ok(outcome.speaker_id.map(|u| u.to_string()))
}

/// Persist a single VAD segment along with its speaker identification.
///
/// Order of operations:
/// 1. If no embedding model is loaded: insert the row with NULL speaker +
///    NULL embedding so the user can still retroactively tag it.
/// 2. Extract the embedding; on failure, same outcome as step 1.
/// 3. Live-enrollment short-circuit: synthetic Confirmed publish, no
///    continuity touch.
/// 4. Duration gate: very short segments pass Unknown through continuity
///    so a carry-over can rescue the attribution.
/// 5. Run `identify_speaker_with_thresholds`; funnel result through
///    `finalize_segment` which drives the continuity state machine.
#[allow(clippy::too_many_arguments)]
async fn handle_segment_embedding(
    pool: &sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    session_id: Uuid,
    tenant_id: Uuid,
    segment_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    audio: Vec<f32>,
    clips_dir: &Path,
    live_enrollment: &LiveEnrollment,
    aggregator: &Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: &Arc<Mutex<ContinuityState>>,
) -> anyhow::Result<Option<String>> {
    // Raw-publish helper for paths that must not touch continuity.
    let publish_raw = |speaker_id: Option<String>, confidence: Option<&'static str>| {
        aggregator.publish_speaker_resolved(
            crate::engine::transcript_aggregator::SpeakerResolvedEvent {
                segment_id: segment_id.to_string(),
                start_ms,
                end_ms,
                speaker_id,
                confidence,
                carried_over: false,
            },
        );
    };

    let Some(model_path) = embedding_model else {
        info!(
            start_ms,
            end_ms, "segment hook: no embedding model — marking UNKNOWN"
        );
        crate::repository::segment::insert_segment(
            pool, session_id, start_ms, end_ms, None, None, None, None,
        )
        .await?;
        publish_raw(None, None);
        return Ok(None);
    };

    let emb = match crate::engine::diarization::extract_embedding(&model_path, &audio).await {
        Ok(e) => e,
        Err(err) => {
            warn!(?err, "speaker embedding failed; segment marked UNKNOWN");
            crate::repository::segment::insert_segment(
                pool, session_id, start_ms, end_ms, None, None, None, None,
            )
            .await?;
            publish_raw(None, None);
            return Ok(None);
        }
    };

    let duration_ms = (end_ms - start_ms) as f64;
    let quality = crate::engine::audio_quality::score(&audio);

    // Live-enrollment short-circuit: synthetic Confirmed publish, no
    // continuity touch.
    if let Some(enrolled_speaker) = crate::engine::live_enrollment::consume_segment(
        live_enrollment,
        duration_ms,
        quality,
        pool,
        &emb.values,
    )
    .await?
    {
        let speaker_uuid = Uuid::parse_str(&enrolled_speaker).ok();
        crate::repository::segment::insert_segment(
            pool,
            session_id,
            start_ms,
            end_ms,
            speaker_uuid,
            None,
            Some(&emb.values),
            None,
        )
        .await?;
        publish_raw(
            Some(enrolled_speaker.clone()),
            Some(crate::domain::speaker_matcher::MatchConfidence::Confirmed.as_str()),
        );
        return Ok(Some(enrolled_speaker));
    }

    let config = ContinuityConfig {
        window_ms: speaker_id_config.continuity_window_ms,
    };

    // Duration gate: very short VAD segments give noisy embeddings. Skip
    // the identifier entirely, but still pass Unknown through continuity
    // so a carry-over can rescue the attribution.
    if duration_ms < speaker_id_config.min_duration_ms as f64 {
        info!(
            duration_ms,
            min = speaker_id_config.min_duration_ms,
            "segment hook: skipping speaker-id — too short"
        );
        return finalize_segment(
            pool,
            session_id,
            start_ms,
            end_ms,
            Some(&emb.values),
            &audio,
            quality,
            clips_dir,
            MatchEvidence::Unknown,
            None,
            continuity,
            config,
            aggregator,
            segment_id,
        )
        .await;
    }

    info!(
        dim = emb.values.len(),
        start_ms, end_ms, "segment hook: identifying speaker"
    );
    let thresholds = crate::domain::speaker_matcher::IdentifyThresholds {
        confirm: speaker_id_config.confirm_threshold as f64,
        tentative: speaker_id_config.tentative_threshold as f64,
    };
    let result = crate::domain::speaker_matcher::identify_speaker_with_thresholds(
        pool,
        &emb.values,
        tenant_id,
        thresholds,
    )
    .await
    .unwrap_or(crate::domain::speaker_matcher::SpeakerMatchResult {
        speaker_id: None,
        similarity_score: 0.0,
        z_norm_score: 0.0,
        accepted: false,
        confidence: None,
    });

    let evidence = match (
        result.confidence,
        result
            .speaker_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok()),
    ) {
        (Some(crate::domain::speaker_matcher::MatchConfidence::Confirmed), Some(id)) => {
            MatchEvidence::Confirmed { speaker_id: id }
        }
        (Some(crate::domain::speaker_matcher::MatchConfidence::Tentative), Some(id)) => {
            MatchEvidence::Tentative { speaker_id: id }
        }
        _ => MatchEvidence::Unknown,
    };
    let match_similarity = match evidence {
        MatchEvidence::Unknown => None,
        _ => Some(result.similarity_score),
    };

    finalize_segment(
        pool,
        session_id,
        start_ms,
        end_ms,
        Some(&emb.values),
        &audio,
        quality,
        clips_dir,
        evidence,
        match_similarity,
        continuity,
        config,
        aggregator,
        segment_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::continuity::{ContinuityConfig, ContinuityState, MatchEvidence};
    use crate::repository::db::run_migrations;
    use crate::repository::session::create_session as create_session_row;
    use crate::repository::speaker::create_speaker;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::path::PathBuf;

    async fn fresh_pool() -> sqlx::SqlitePool {
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

    #[tokio::test]
    async fn finalize_segment_carries_over_after_confirmed() {
        let pool = fresh_pool().await;
        let speaker = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let speaker_uuid = Uuid::parse_str(&speaker.id).unwrap();

        let aggregator = Arc::new(TranscriptAggregator::new(pool.clone()));
        let mut speaker_rx = aggregator.subscribe_speaker();

        let continuity: Arc<Mutex<ContinuityState>> =
            Arc::new(Mutex::new(ContinuityState::default()));
        let config = ContinuityConfig { window_ms: 15_000 };
        let clips_dir = PathBuf::from(std::env::temp_dir()).join("actio-test-clips");
        std::fs::create_dir_all(&clips_dir).unwrap();
        // audio_segments.session_id has a FK to audio_sessions(id), so the
        // test must create a real session row before inserting segments.
        let session_row = create_session_row(&pool, Uuid::nil(), "microphone", "test")
            .await
            .unwrap();
        let session_id = Uuid::parse_str(&session_row.id).unwrap();
        let audio: Vec<f32> = vec![0.0; 16_000]; // 1s of silence — won't trigger clip retention
        let embedding: Vec<f32> = vec![0.1_f32; 192];

        // 1. Confirmed match for our speaker at segment ending at 3_000 ms.
        let seg_id_1 = Uuid::new_v4();
        finalize_segment(
            &pool,
            session_id,
            0,
            3_000,
            Some(&embedding),
            &audio,
            0.5, // quality below retention threshold — no clip write
            &clips_dir,
            MatchEvidence::Confirmed {
                speaker_id: speaker_uuid,
            },
            Some(0.72),
            &continuity,
            config,
            &aggregator,
            seg_id_1,
        )
        .await
        .unwrap();

        // 2. Unknown evidence 5_000 ms later — well within 15_000 window.
        let seg_id_2 = Uuid::new_v4();
        finalize_segment(
            &pool,
            session_id,
            3_000,
            8_000,
            Some(&embedding),
            &audio,
            0.5,
            &clips_dir,
            MatchEvidence::Unknown,
            None,
            &continuity,
            config,
            &aggregator,
            seg_id_2,
        )
        .await
        .unwrap();

        // Assert the persisted rows.
        let rows: Vec<(String, Option<String>, Option<f64>)> = sqlx::query_as(
            "SELECT id, speaker_id, speaker_score FROM audio_segments \
             WHERE session_id = ?1 ORDER BY start_ms",
        )
        .bind(session_id.to_string())
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2, "two segments should be persisted");

        let row1 = &rows[0];
        assert_eq!(
            row1.1.as_deref(),
            Some(speaker.id.as_str()),
            "Confirmed row should carry the matched speaker id"
        );
        assert!(
            row1.2.is_some(),
            "Confirmed row should persist the similarity"
        );
        assert!((row1.2.unwrap() - 0.72).abs() < 1e-6);

        let row2 = &rows[1];
        assert_eq!(
            row2.1.as_deref(),
            Some(speaker.id.as_str()),
            "Unknown-within-window row should carry over the previous speaker"
        );
        assert!(
            row2.2.is_none(),
            "carry-over rows must persist speaker_score = NULL"
        );

        // Assert both speaker_resolved events fired with the expected flags.
        let ev1 = speaker_rx
            .try_recv()
            .expect("first event should be buffered");
        assert_eq!(ev1.speaker_id.as_deref(), Some(speaker.id.as_str()));
        assert_eq!(ev1.confidence, Some("confirmed"));
        assert!(!ev1.carried_over);

        let ev2 = speaker_rx
            .try_recv()
            .expect("second event should be buffered");
        assert_eq!(ev2.speaker_id.as_deref(), Some(speaker.id.as_str()));
        assert_eq!(ev2.confidence, Some("tentative"));
        assert!(
            ev2.carried_over,
            "second event should be flagged as carried over"
        );

        // Continuity state should still point at Alice and still hold the
        // Confirmed timestamp (carry-over did not self-extend).
        let state = continuity.lock().await;
        assert_eq!(state.speaker_id, Some(speaker_uuid));
        assert_eq!(state.last_confirmed_ms, Some(3_000));
    }

}
