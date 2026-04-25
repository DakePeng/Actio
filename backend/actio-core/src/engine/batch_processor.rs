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
