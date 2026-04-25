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

    async fn fresh_pool() -> SqlitePool {
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

    async fn mk_session(pool: &SqlitePool) -> Uuid {
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
