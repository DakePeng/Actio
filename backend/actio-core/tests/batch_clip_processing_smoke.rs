//! End-to-end batch pipeline smoke test using stub ASR + stub embedder.
//!
//! Verifies the cross-clip provisional linking guarantee from the spec:
//! a person who appears in two consecutive clips with similar embeddings
//! must be assigned the same `speaker_id` in both — automatically, with
//! no enrollment step. This is the "option C" coherence promise.

use std::path::Path;
use std::sync::Arc;

use actio_core::domain::types::{ClipManifest, ClipManifestSegment};
use actio_core::engine::batch_processor::{
    process_clip_with_clustering, ArchiveAsr, ArchiveTranscript, ClusteringConfig, SegmentEmbedder,
};
use actio_core::engine::clip_writer::write_manifest;
use actio_core::repository::{audio_clip, db, segment, speaker};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tempfile::tempdir;
use uuid::Uuid;

async fn fresh_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap();
    db::run_migrations(&pool).await.unwrap();
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
        _audio_dir: &Path,
    ) -> anyhow::Result<Vec<ArchiveTranscript>> {
        Ok(manifest
            .segments
            .iter()
            .map(|s| ArchiveTranscript {
                segment_id: s.id,
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: format!("hello {}", s.start_ms),
            })
            .collect())
    }
}

struct StubEmbedder {
    vecs: Vec<Vec<f32>>,
    dim: i64,
}
impl SegmentEmbedder for StubEmbedder {
    fn embed_segments(
        &self,
        manifest: &ClipManifest,
        _audio_dir: &Path,
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

fn cfg() -> ClusteringConfig {
    ClusteringConfig {
        cosine_threshold: 0.4,
        min_segments_per_cluster: 1,
        min_duration_ms: 0,
        confirm_threshold: 0.55,
    }
}

#[tokio::test]
async fn two_clips_with_same_speaker_link_via_provisional() {
    let pool = fresh_pool().await;
    let session_id = mk_session(&pool).await;

    // Clip 1: single segment with embedding [1.0, 0.0]. No prior speakers
    // exist → centroid match returns None → new provisional row inserted.
    let tmp1 = tempdir().unwrap();
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
    let p1 = write_manifest(tmp1.path(), &m1).unwrap();
    audio_clip::insert_pending(
        &pool,
        session_id,
        0,
        300_000,
        1,
        p1.to_string_lossy().as_ref(),
    )
    .await
    .unwrap();
    let c1 = audio_clip::claim_next_pending(&pool)
        .await
        .unwrap()
        .unwrap();
    process_clip_with_clustering(
        &pool,
        Arc::new(StubAsr),
        Arc::new(StubEmbedder {
            vecs: vec![vec![1.0, 0.0]],
            dim: 2,
        }),
        &c1,
        &cfg(),
        None,
    )
    .await
    .unwrap();

    // Clip 2: single segment with near-collinear embedding [0.99, 0.14].
    // Centroid matches the first clip's provisional row → no new row;
    // the same speaker_id propagates.
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
    let c2 = audio_clip::claim_next_pending(&pool)
        .await
        .unwrap()
        .unwrap();
    process_clip_with_clustering(
        &pool,
        Arc::new(StubAsr),
        Arc::new(StubEmbedder {
            vecs: vec![vec![0.99, 0.14]],
            dim: 2,
        }),
        &c2,
        &cfg(),
        None,
    )
    .await
    .unwrap();

    // Spec guarantee: exactly one provisional row, both clips share speaker_id.
    let provisionals = speaker::list_provisional(&pool).await.unwrap();
    assert_eq!(
        provisionals.len(),
        1,
        "second clip should reuse the first's provisional row, not create a new one"
    );

    let segs1 = segment::list_for_clip(&pool, c1.id).await.unwrap();
    let segs2 = segment::list_for_clip(&pool, c2.id).await.unwrap();
    assert_eq!(segs1.len(), 1);
    assert_eq!(segs2.len(), 1);
    assert!(segs1[0].speaker_id.is_some());
    assert_eq!(
        segs1[0].speaker_id, segs2[0].speaker_id,
        "same person across clips → same speaker_id"
    );

    // Both clips marked processed.
    for c in [c1.id, c2.id] {
        let row = audio_clip::get_by_id(&pool, c).await.unwrap().unwrap();
        assert_eq!(row.status, "processed");
    }
}

#[tokio::test]
async fn promote_provisional_after_clip_makes_it_indistinguishable_from_enrolled() {
    let pool = fresh_pool().await;
    let session_id = mk_session(&pool).await;

    let tmp = tempdir().unwrap();
    let m = ClipManifest {
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
    let p = write_manifest(tmp.path(), &m).unwrap();
    audio_clip::insert_pending(
        &pool,
        session_id,
        0,
        300_000,
        1,
        p.to_string_lossy().as_ref(),
    )
    .await
    .unwrap();
    let c = audio_clip::claim_next_pending(&pool)
        .await
        .unwrap()
        .unwrap();
    process_clip_with_clustering(
        &pool,
        Arc::new(StubAsr),
        Arc::new(StubEmbedder {
            vecs: vec![vec![1.0, 0.0]],
            dim: 2,
        }),
        &c,
        &cfg(),
        None,
    )
    .await
    .unwrap();

    // The provisional row exists.
    let provisionals = speaker::list_provisional(&pool).await.unwrap();
    assert_eq!(provisionals.len(), 1);
    let provisional_id = Uuid::parse_str(&provisionals[0].id).unwrap();

    // User promotes via the candidate-speakers panel.
    let promoted = speaker::promote_provisional(&pool, provisional_id, Some("Alice"))
        .await
        .unwrap();
    assert!(promoted);

    // Provisional list is empty; the speaker survives as enrolled with the new name.
    let after = speaker::list_provisional(&pool).await.unwrap();
    assert!(
        after.is_empty(),
        "promoted row should leave the provisional pool"
    );
    let row: (String, String) =
        sqlx::query_as("SELECT display_name, kind FROM speakers WHERE id = ?1")
            .bind(provisional_id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "Alice");
    assert_eq!(row.1, "enrolled");
}
