# Batch Clip Processing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the always-on inference pipeline with a thin always-on capture daemon, a deferred batch processor that produces the canonical archive transcripts (with per-clip global-clustering diarization + auto-provisional speakers), and a separate on-demand live streaming service for dictation/translation.

**Architecture:** Three independent subsystems share one `cpal` capture stream. The CaptureDaemon writes per-VAD-segment WAVs to disk and broadcasts to optional live subscribers. ClipBoundaryWatcher closes a clip on the first ≥1.5 s silence past the 5-min mark. BatchProcessor cold-loads the archive ASR, transcribes, clusters embeddings (AHC), matches centroids against `speakers` (kind='enrolled' or 'provisional'), creates auto-provisional rows for unmatched clusters, and triggers post-clip action-item extraction. LiveStreamingService spins up per-segment ASR + the existing continuity machine only while dictation/translation is active and writes nothing to the DB.

**Tech Stack:** Rust 2021, sqlx (SQLite), cpal, sherpa-onnx (ASR + speaker embeddings), tokio, hound (WAV I/O), Axum/utoipa for HTTP, serde for manifest I/O.

**Spec:** `docs/superpowers/specs/2026-04-25-batch-clip-processing-design.md`

**Translation note vs. spec:** The spec refers to a "voiceprints" table, which does not exist in this repo — embeddings are stored on `audio_segments.embedding` and "voiceprints" is conceptual. This plan adds `speakers.kind` (`'enrolled' | 'provisional'`) and `speakers.provisional_last_matched_at` instead. A provisional voiceprint = a `speakers` row with `kind='provisional'`, holding cluster-member segments via `audio_segments.speaker_id`.

---

## File Map

**New files:**
- `backend/actio-core/migrations/005_batch_clip_processing.sql`
- `backend/actio-core/src/engine/cluster.rs` — pure AHC over embeddings
- `backend/actio-core/src/engine/clip_boundary.rs` — boundary state machine
- `backend/actio-core/src/engine/capture_daemon.rs` — cpal + VAD daemon
- `backend/actio-core/src/engine/batch_processor.rs` — clip-level batch worker
- `backend/actio-core/src/engine/live_streaming.rs` — on-demand streaming service
- `backend/actio-core/src/repository/audio_clip.rs` — audio_clips CRUD
- `backend/actio-core/src/api/candidate_speaker.rs` — Candidate Speakers panel endpoints
- `docs/superpowers/specs/2026-04-25-batch-clip-processing-design.md` (already committed)

**Modified files:**
- `backend/actio-core/src/engine/app_settings.rs` — new audio settings, deprecate three
- `backend/actio-core/src/engine/window_extractor.rs` — driven by post-clip hook, not its own scheduler
- `backend/actio-core/src/engine/clip_storage.rs` — generalized 14-day sweep
- `backend/actio-core/src/engine/mod.rs` — register new modules
- `backend/actio-core/src/repository/mod.rs` — register `audio_clip`
- `backend/actio-core/src/repository/speaker.rs` — kind, provisional GC helpers
- `backend/actio-core/src/lib.rs` — pipeline_supervisor refactor, retire IDLE_GRACE_PERIOD
- `backend/actio-core/src/api/reminder.rs` — trace endpoint fallback
- `backend/actio-core/src/api/mod.rs` — register candidate_speaker routes
- `backend/actio-core/src/domain/types.rs` — AudioClip, ClipManifest, SpeakerKind
- `backend/actio-core/src/domain/mod.rs` — re-exports
- `backend/actio-core/src/state.rs` (or wherever AppState lives) — add batch processor handle, capture daemon handle

**Retired (delete or shrink to a single rexport stub):**
- `backend/actio-core/src/engine/inference_pipeline.rs` — internals migrate to `live_streaming.rs`; the file becomes a thin re-export until call sites are updated, then deletes.

---

## Task 1: SQL migration 005 — audio_clips, speakers.kind, audio_segments.clip_id

**Files:**
- Create: `backend/actio-core/migrations/005_batch_clip_processing.sql`
- Test: `backend/actio-core/src/repository/audio_clip.rs` (test module — created in Task 3, but the migration is exercised by the existing `repository::db::run_migrations` boot path).

- [ ] **Step 1: Write the migration**

```sql
-- Migration 005: batch clip processing pipeline.
--
-- Replaces the streaming-derived persistence model. Each ~5-min audio clip
-- is now an explicit row with a manifest pointing at its on-disk per-VAD
-- segment WAVs. Transcripts and speaker assignments come from a deferred
-- batch pass over the clip; per-clip global clustering produces stable
-- attribution without requiring enrollment.

-- 1) Clips table. status cycles pending → running → processed | empty | failed.
CREATE TABLE audio_clips (
    id              TEXT PRIMARY KEY NOT NULL,
    session_id      TEXT NOT NULL REFERENCES audio_sessions(id) ON DELETE CASCADE,
    started_at_ms   INTEGER NOT NULL,
    ended_at_ms     INTEGER NOT NULL,
    segment_count   INTEGER NOT NULL,
    manifest_path   TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                      CHECK (status IN ('pending','running','processed','empty','failed')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    archive_model   TEXT,
    last_error      TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at     TEXT
);
CREATE INDEX idx_audio_clips_status ON audio_clips(status);
CREATE INDEX idx_audio_clips_session ON audio_clips(session_id, started_at_ms);

-- 2) Per-segment clip linkage + clip-local speaker index for "Speaker A/B" UI.
ALTER TABLE audio_segments
    ADD COLUMN clip_id TEXT REFERENCES audio_clips(id);
ALTER TABLE audio_segments
    ADD COLUMN clip_local_speaker_idx INTEGER;
CREATE INDEX idx_segments_clip ON audio_segments(clip_id);

-- 3) Speaker kind + provisional GC timestamp. Existing rows are 'enrolled'.
ALTER TABLE speakers
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'enrolled'
        CHECK (kind IN ('enrolled','provisional'));
ALTER TABLE speakers
    ADD COLUMN provisional_last_matched_at TEXT;
CREATE INDEX idx_speakers_provisional
    ON speakers(provisional_last_matched_at)
    WHERE kind = 'provisional';
```

- [ ] **Step 2: Run cargo check and verify migration loads**

```bash
cd backend
cargo check -p actio-core --tests
```

Expected: clean compile. The migration file is picked up by `sqlx::migrate!` at startup; the next test that boots an in-memory DB will exercise it.

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/migrations/005_batch_clip_processing.sql
git commit -m "feat(db): migration 005 — audio_clips, speakers.kind, segment.clip_id"
```

---

## Task 2: AudioSettings additions and deprecations

**Files:**
- Modify: `backend/actio-core/src/engine/app_settings.rs`

- [ ] **Step 1: Write a failing test for the new defaults**

Append to the test module in `app_settings.rs`:

```rust
#[test]
fn audio_settings_defaults_have_clip_processing_fields() {
    let s = AudioSettings::default();
    assert_eq!(s.clip_target_secs, 300);
    assert_eq!(s.clip_max_secs, 360);
    assert_eq!(s.clip_close_silence_ms, 1500);
    assert!((s.cluster_cosine_threshold - 0.4).abs() < 1e-6);
    assert_eq!(s.audio_retention_days, 14);
    assert_eq!(s.provisional_voiceprint_gc_days, 30);
    assert_eq!(s.live_asr_model, None);
    assert_eq!(s.archive_asr_model, None);
}
```

Run: `cargo test -p actio-core --lib audio_settings_defaults_have_clip_processing_fields`
Expected: FAIL — fields don't exist.

- [ ] **Step 2: Add the fields and defaults**

In `AudioSettings`, add:

```rust
    /// Per-mode ASR model selection. `live_asr_model` drives dictation/
    /// translation; `archive_asr_model` drives the batch processor. Both
    /// fall back to the legacy `asr_model` if unset (read-time migration).
    #[serde(default)]
    pub live_asr_model: Option<String>,
    #[serde(default)]
    pub archive_asr_model: Option<String>,

    /// Target clip duration in seconds before the boundary watcher starts
    /// looking for a silence to close on. Default 300 (5 min).
    #[serde(default = "default_clip_target_secs")]
    pub clip_target_secs: u32,
    /// Hard cap — clip force-closes at this duration even mid-utterance.
    #[serde(default = "default_clip_max_secs")]
    pub clip_max_secs: u32,
    /// Minimum VAD silence duration to count as a clip boundary, once past
    /// `clip_target_secs`. Default 1500 ms.
    #[serde(default = "default_clip_close_silence_ms")]
    pub clip_close_silence_ms: u32,

    /// AHC cosine threshold inside `cluster::ahc`. Smaller = more clusters.
    #[serde(default = "default_cluster_cosine_threshold")]
    pub cluster_cosine_threshold: f32,

    /// Per-clip WAV files older than this many days are swept by the
    /// background cleanup task. Replaces the per-failed-segment retention
    /// path that used `clip_retention_days`.
    #[serde(default = "default_audio_retention_days")]
    pub audio_retention_days: u32,
    /// Provisional speakers (kind='provisional') with no match in this many
    /// days are GC'd (DELETE cascades their attached segments' speaker_id).
    #[serde(default = "default_provisional_voiceprint_gc_days")]
    pub provisional_voiceprint_gc_days: u32,
```

And the corresponding default fns:

```rust
fn default_clip_target_secs() -> u32 { 300 }
fn default_clip_max_secs() -> u32 { 360 }
fn default_clip_close_silence_ms() -> u32 { 1500 }
fn default_cluster_cosine_threshold() -> f32 { 0.4 }
fn default_audio_retention_days() -> u32 { 14 }
fn default_provisional_voiceprint_gc_days() -> u32 { 30 }
```

Update the `Default for AudioSettings` impl and `AudioSettingsPatch` struct to include each field. Keep `window_length_ms`, `window_step_ms`, `extraction_tick_secs`, `clip_retention_days`, `asr_model` for now — they're read-only deprecated and removed in Task 17.

Run: `cargo test -p actio-core --lib audio_settings_defaults_have_clip_processing_fields`
Expected: PASS.

- [ ] **Step 3: Add a read-time migration test for the legacy `asr_model` field**

```rust
#[test]
fn live_and_archive_asr_default_to_legacy_asr_model_when_unset() {
    use crate::engine::app_settings::AudioSettings;
    let mut s = AudioSettings::default();
    s.asr_model = Some("zipformer-en".to_string());
    s.live_asr_model = None;
    s.archive_asr_model = None;
    let resolved = s.resolved_asr_models();
    assert_eq!(resolved.live.as_deref(), Some("zipformer-en"));
    assert_eq!(resolved.archive.as_deref(), Some("zipformer-en"));
}
```

Run: `cargo test -p actio-core --lib live_and_archive_asr_default_to_legacy_asr_model_when_unset`
Expected: FAIL — `resolved_asr_models` does not exist.

- [ ] **Step 4: Implement `resolved_asr_models`**

Add to `AudioSettings`:

```rust
pub struct ResolvedAsrModels {
    pub live: Option<String>,
    pub archive: Option<String>,
}

impl AudioSettings {
    /// Resolve the live and archive ASR model selections, falling back to
    /// the legacy `asr_model` when either is unset. Single source of truth
    /// for callers that need to know which model to load.
    pub fn resolved_asr_models(&self) -> ResolvedAsrModels {
        ResolvedAsrModels {
            live: self.live_asr_model.clone().or_else(|| self.asr_model.clone()),
            archive: self.archive_asr_model.clone().or_else(|| self.asr_model.clone()),
        }
    }
}
```

Run: `cargo test -p actio-core --lib live_and_archive_asr_default_to_legacy_asr_model_when_unset`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/app_settings.rs
git commit -m "feat(settings): add live/archive asr models, clip + retention knobs"
```

---

## Task 3: domain types + audio_clip repository

**Files:**
- Modify: `backend/actio-core/src/domain/types.rs`
- Modify: `backend/actio-core/src/domain/mod.rs`
- Create: `backend/actio-core/src/repository/audio_clip.rs`
- Modify: `backend/actio-core/src/repository/mod.rs`

- [ ] **Step 1: Write a failing repository test**

Create `backend/actio-core/src/repository/audio_clip.rs` with the test module skeleton:

```rust
//! Audio clip persistence — one row per ~5-min recorded clip on disk.
//! Status cycles `pending → running → processed | empty | failed`. Crash
//! recovery: `requeue_stale_running` reverts orphans on startup.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use crate::domain::types::AudioClip;

#[derive(Debug, Clone, FromRow)]
pub struct AudioClipRow {
    pub id: String,
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub segment_count: i64,
    pub manifest_path: String,
    pub status: String,
    pub attempts: i64,
    pub archive_model: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

impl AudioClipRow {
    pub fn into_clip(self) -> AudioClip {
        AudioClip {
            id: Uuid::parse_str(&self.id).unwrap_or_default(),
            session_id: Uuid::parse_str(&self.session_id).unwrap_or_default(),
            started_at_ms: self.started_at_ms,
            ended_at_ms: self.ended_at_ms,
            segment_count: self.segment_count,
            manifest_path: self.manifest_path,
            status: self.status,
            attempts: self.attempts,
            archive_model: self.archive_model,
            last_error: self.last_error,
            created_at: DateTime::parse_from_rfc3339(&self.created_at)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            finished_at: self.finished_at.and_then(|s| {
                DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::test_helpers::test_pool;
    use crate::repository::session;

    #[tokio::test]
    async fn insert_pending_then_claim_marks_running() {
        let pool = test_pool().await;
        let session_id = session::create_default_session(&pool).await.unwrap();

        let id = insert_pending(
            &pool,
            session_id,
            1_000,
            301_000,
            5,
            "/tmp/foo/manifest.json",
        )
        .await
        .unwrap();

        let claimed = claim_next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(claimed.id, id);
        assert_eq!(claimed.status, "running");
        assert_eq!(claimed.attempts, 1);

        // Idempotency — no second claim.
        assert!(claim_next_pending(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn requeue_stale_running_reverts_orphans() {
        let pool = test_pool().await;
        let session_id = session::create_default_session(&pool).await.unwrap();
        let id = insert_pending(&pool, session_id, 0, 300_000, 3, "/tmp/m.json")
            .await
            .unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();

        let n = requeue_stale_running(&pool).await.unwrap();
        assert_eq!(n, 1);
        let clip = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(clip.status, "pending");
    }

    #[tokio::test]
    async fn mark_processed_sets_finished_at() {
        let pool = test_pool().await;
        let session_id = session::create_default_session(&pool).await.unwrap();
        let id = insert_pending(&pool, session_id, 0, 300_000, 3, "/tmp/m.json")
            .await
            .unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();
        mark_processed(&pool, id, Some("whisper-medium")).await.unwrap();
        let clip = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(clip.status, "processed");
        assert!(clip.finished_at.is_some());
        assert_eq!(clip.archive_model.as_deref(), Some("whisper-medium"));
    }
}
```

Run: `cargo test -p actio-core --lib audio_clip`
Expected: FAIL — symbols don't exist (`insert_pending`, `claim_next_pending`, etc.).

- [ ] **Step 2: Add `AudioClip` and `SpeakerKind` to `domain/types.rs`**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerKind {
    Enrolled,
    Provisional,
}

impl SpeakerKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpeakerKind::Enrolled => "enrolled",
            SpeakerKind::Provisional => "provisional",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "enrolled" => Some(SpeakerKind::Enrolled),
            "provisional" => Some(SpeakerKind::Provisional),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioClip {
    pub id: Uuid,
    pub session_id: Uuid,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub segment_count: i64,
    pub manifest_path: String,
    pub status: String,
    pub attempts: i64,
    pub archive_model: Option<String>,
    pub last_error: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}
```

Re-export from `domain/mod.rs` if a `pub use types::*` pattern isn't already in place.

- [ ] **Step 3: Implement the repository functions**

Append to `backend/actio-core/src/repository/audio_clip.rs`:

```rust
pub async fn insert_pending(
    pool: &SqlitePool,
    session_id: Uuid,
    started_at_ms: i64,
    ended_at_ms: i64,
    segment_count: i64,
    manifest_path: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO audio_clips
           (id, session_id, started_at_ms, ended_at_ms, segment_count,
            manifest_path, status, attempts)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0)"#,
    )
    .bind(id.to_string())
    .bind(session_id.to_string())
    .bind(started_at_ms)
    .bind(ended_at_ms)
    .bind(segment_count)
    .bind(manifest_path)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn claim_next_pending(
    pool: &SqlitePool,
) -> Result<Option<AudioClip>, sqlx::Error> {
    let row: Option<AudioClipRow> = sqlx::query_as(
        r#"UPDATE audio_clips
           SET status = 'running', attempts = attempts + 1
           WHERE id = (
               SELECT id FROM audio_clips
               WHERE status = 'pending' AND attempts < 3
               ORDER BY started_at_ms ASC
               LIMIT 1
           )
           RETURNING *"#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(AudioClipRow::into_clip))
}

pub async fn requeue_stale_running(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"UPDATE audio_clips SET status = 'pending'
           WHERE status = 'running'"#,
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn mark_processed(
    pool: &SqlitePool,
    id: Uuid,
    archive_model: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE audio_clips
           SET status = 'processed',
               archive_model = COALESCE(?2, archive_model),
               finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .bind(archive_model)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_empty(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE audio_clips
           SET status = 'empty',
               finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_failed(pool: &SqlitePool, id: Uuid, err: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE audio_clips
           SET status = CASE WHEN attempts >= 3 THEN 'failed' ELSE 'pending' END,
               last_error = ?2,
               finished_at = CASE WHEN attempts >= 3
                   THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE finished_at END
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .bind(err)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_by_id(
    pool: &SqlitePool,
    id: Uuid,
) -> Result<Option<AudioClip>, sqlx::Error> {
    let row: Option<AudioClipRow> = sqlx::query_as(
        r#"SELECT * FROM audio_clips WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(AudioClipRow::into_clip))
}
```

Add `pub mod audio_clip;` to `backend/actio-core/src/repository/mod.rs`.

- [ ] **Step 4: Run the tests**

```bash
cargo test -p actio-core --lib audio_clip
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/domain/types.rs \
        backend/actio-core/src/domain/mod.rs \
        backend/actio-core/src/repository/audio_clip.rs \
        backend/actio-core/src/repository/mod.rs
git commit -m "feat(repo): audio_clip CRUD with claim/requeue/mark_* helpers"
```

---

## Task 4: cluster.rs — pure agglomerative hierarchical clustering

**Files:**
- Create: `backend/actio-core/src/engine/cluster.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

- [ ] **Step 1: Write failing tests for clustering correctness**

Create `backend/actio-core/src/engine/cluster.rs`:

```rust
//! Agglomerative hierarchical clustering over speaker embeddings.
//!
//! Pure function: input is `(segment_id, embedding)` pairs and a cosine
//! distance threshold; output is a stable cluster index per input. Used by
//! the batch processor to derive per-clip speaker tracks.
//!
//! Algorithm: average-linkage AHC on the upper triangle of the cosine-
//! distance matrix. Stops merging when the smallest pair distance exceeds
//! `threshold`. O(n^2 log n) but n ≤ a few hundred per 5-min clip.

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterAssignment {
    pub segment_id: Uuid,
    pub cluster_idx: usize,
}

pub fn ahc(
    inputs: &[(Uuid, Vec<f32>)],
    cosine_distance_threshold: f32,
) -> Vec<ClusterAssignment> {
    if inputs.is_empty() {
        return Vec::new();
    }
    if inputs.len() == 1 {
        return vec![ClusterAssignment { segment_id: inputs[0].0, cluster_idx: 0 }];
    }

    let n = inputs.len();
    // Each cluster is a set of input indices. Membership[i] = cluster id.
    let mut membership: Vec<usize> = (0..n).collect();
    let mut active: std::collections::BTreeSet<usize> = (0..n).collect();
    let mut sizes: Vec<usize> = vec![1; n];
    let mut centroids: Vec<Vec<f32>> =
        inputs.iter().map(|(_, v)| normalized(v)).collect();

    loop {
        let mut best: Option<(f32, usize, usize)> = None;
        let actives: Vec<usize> = active.iter().copied().collect();
        for i in 0..actives.len() {
            for j in (i + 1)..actives.len() {
                let a = actives[i];
                let b = actives[j];
                let d = 1.0 - cosine_sim(&centroids[a], &centroids[b]);
                if best.map_or(true, |(bd, _, _)| d < bd) {
                    best = Some((d, a, b));
                }
            }
        }
        match best {
            Some((d, a, b)) if d <= cosine_distance_threshold => {
                // Merge b into a. Update membership, centroid, sizes, active set.
                let new_centroid = weighted_mean(
                    &centroids[a], sizes[a],
                    &centroids[b], sizes[b],
                );
                let new_size = sizes[a] + sizes[b];
                centroids[a] = normalized(&new_centroid);
                sizes[a] = new_size;
                for m in membership.iter_mut() {
                    if *m == b {
                        *m = a;
                    }
                }
                active.remove(&b);
            }
            _ => break,
        }
    }

    // Compact membership into 0..k.
    let mut compact: std::collections::BTreeMap<usize, usize> = Default::default();
    for &m in membership.iter() {
        let next = compact.len();
        compact.entry(m).or_insert(next);
    }
    inputs.iter().enumerate().map(|(i, (id, _))| ClusterAssignment {
        segment_id: *id,
        cluster_idx: compact[&membership[i]],
    }).collect()
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>()
}

fn norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn normalized(v: &[f32]) -> Vec<f32> {
    let n = norm(v);
    if n < 1e-8 { v.to_vec() } else { v.iter().map(|x| x / n).collect() }
}

fn weighted_mean(a: &[f32], an: usize, b: &[f32], bn: usize) -> Vec<f32> {
    let total = (an + bn) as f32;
    a.iter().zip(b.iter()).map(|(x, y)| {
        (*x * an as f32 + *y * bn as f32) / total
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(ahc(&[], 0.4).is_empty());
    }

    #[test]
    fn single_input_returns_one_cluster() {
        let out = ahc(&[(id(1), vec![1.0, 0.0])], 0.4);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].cluster_idx, 0);
    }

    #[test]
    fn two_orthogonal_vectors_are_two_clusters() {
        let inputs = vec![(id(1), vec![1.0, 0.0]), (id(2), vec![0.0, 1.0])];
        let out = ahc(&inputs, 0.4);
        assert_eq!(out[0].cluster_idx, 0);
        assert_eq!(out[1].cluster_idx, 1);
    }

    #[test]
    fn two_collinear_vectors_collapse_into_one_cluster() {
        let inputs = vec![(id(1), vec![1.0, 0.0]), (id(2), vec![0.999, 0.044])];
        let out = ahc(&inputs, 0.4);
        assert_eq!(out[0].cluster_idx, out[1].cluster_idx);
    }

    #[test]
    fn three_speakers_two_clusters_each_resolves_correctly() {
        // Two near-collinear pairs (speaker A, B) plus an isolate (speaker C).
        let inputs = vec![
            (id(1), vec![1.0, 0.0, 0.0]),
            (id(2), vec![0.99, 0.14, 0.0]),    // A
            (id(3), vec![0.0, 1.0, 0.0]),
            (id(4), vec![0.14, 0.99, 0.0]),    // B
            (id(5), vec![0.0, 0.0, 1.0]),       // C
        ];
        let out = ahc(&inputs, 0.4);
        assert_eq!(out[0].cluster_idx, out[1].cluster_idx);
        assert_eq!(out[2].cluster_idx, out[3].cluster_idx);
        assert_ne!(out[0].cluster_idx, out[2].cluster_idx);
        assert_ne!(out[0].cluster_idx, out[4].cluster_idx);
        assert_ne!(out[2].cluster_idx, out[4].cluster_idx);
    }

    #[test]
    fn stable_cluster_indices_are_compact_zero_indexed() {
        let inputs = vec![
            (id(1), vec![1.0, 0.0]),
            (id(2), vec![0.0, 1.0]),
            (id(3), vec![0.99, 0.14]),
        ];
        let out = ahc(&inputs, 0.4);
        let max_idx = out.iter().map(|a| a.cluster_idx).max().unwrap();
        assert!(max_idx < out.len());
        let used: std::collections::BTreeSet<_> =
            out.iter().map(|a| a.cluster_idx).collect();
        assert_eq!(used.len(), max_idx + 1);
    }
}
```

Add `pub mod cluster;` to `backend/actio-core/src/engine/mod.rs`.

Run: `cargo test -p actio-core --lib cluster`
Expected: 6 tests pass.

- [ ] **Step 2: Run cargo clippy on cluster.rs**

```bash
cargo clippy -p actio-core --lib -- -D warnings
```

Expected: clean (or only pre-existing repo-wide lints).

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/src/engine/cluster.rs \
        backend/actio-core/src/engine/mod.rs
git commit -m "feat(engine): pure AHC clustering for per-clip speaker tracks"
```

---

## Task 5: clip_boundary.rs — VAD boundary state machine

**Files:**
- Create: `backend/actio-core/src/engine/clip_boundary.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

The boundary watcher consumes a stream of `BoundaryEvent { Speech { start_ms, end_ms } | Silence { until_ms } | Mute }` and emits a `CloseClip` whenever close conditions hit. Pure state machine — testable without cpal.

- [ ] **Step 1: Write failing tests**

Create `backend/actio-core/src/engine/clip_boundary.rs`:

```rust
//! Pure state machine that decides when to close an audio clip.
//!
//! Reads VAD events as they arrive (speech segments + silence updates) and
//! emits a single `Decision::CloseClip` whenever any of:
//!   * the active clip has been open ≥ `target_secs` AND we observed
//!     ≥ `silence_close_ms` of contiguous silence,
//!   * the active clip has been open ≥ `max_secs` (hard cap, mid-utterance),
//!   * the user toggled mute (capture is stopping).
//!
//! Owns no I/O; the caller plumbs cpal/VAD events in and turns decisions
//! into manifest writes.

#[derive(Debug, Clone, Copy)]
pub struct BoundaryConfig {
    pub target_secs: u32,
    pub max_secs: u32,
    pub silence_close_ms: u32,
}

#[derive(Debug, Clone)]
pub enum BoundaryEvent {
    /// A finalized VAD speech segment.
    Speech { start_ms: i64, end_ms: i64 },
    /// "We are still in silence at this monotonic timestamp." The watcher
    /// uses this to advance the silence-duration counter when no events
    /// would otherwise arrive (idle mic).
    SilenceTick { now_ms: i64 },
    /// User muted — close immediately, even if shorter than `target_secs`.
    Mute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Continue,
    CloseClip,
}

pub struct BoundaryWatcher {
    cfg: BoundaryConfig,
    clip_started_ms: Option<i64>,
    last_speech_end_ms: Option<i64>,
}

impl BoundaryWatcher {
    pub fn new(cfg: BoundaryConfig) -> Self {
        Self { cfg, clip_started_ms: None, last_speech_end_ms: None }
    }

    /// Process one event. Caller must follow up `CloseClip` with a fresh
    /// watcher (or call `reset_after_close`) so the next clip starts fresh.
    pub fn observe(&mut self, ev: BoundaryEvent) -> Decision {
        match ev {
            BoundaryEvent::Speech { start_ms, end_ms } => {
                if self.clip_started_ms.is_none() {
                    self.clip_started_ms = Some(start_ms);
                }
                self.last_speech_end_ms = Some(end_ms);
                self.check_close(end_ms)
            }
            BoundaryEvent::SilenceTick { now_ms } => self.check_close(now_ms),
            BoundaryEvent::Mute => {
                if self.clip_started_ms.is_some() {
                    Decision::CloseClip
                } else {
                    Decision::Continue
                }
            }
        }
    }

    pub fn reset_after_close(&mut self) {
        self.clip_started_ms = None;
        self.last_speech_end_ms = None;
    }

    fn check_close(&self, now_ms: i64) -> Decision {
        let started = match self.clip_started_ms { Some(v) => v, None => return Decision::Continue };
        let elapsed_ms = now_ms - started;
        if elapsed_ms >= self.cfg.max_secs as i64 * 1_000 {
            return Decision::CloseClip;
        }
        if elapsed_ms >= self.cfg.target_secs as i64 * 1_000 {
            let silence = match self.last_speech_end_ms {
                Some(end) => now_ms - end,
                None => elapsed_ms, // entire clip has been silence
            };
            if silence >= self.cfg.silence_close_ms as i64 {
                return Decision::CloseClip;
            }
        }
        Decision::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> BoundaryConfig {
        BoundaryConfig { target_secs: 300, max_secs: 360, silence_close_ms: 1500 }
    }

    #[test]
    fn no_speech_yet_no_close() {
        let mut w = BoundaryWatcher::new(cfg());
        assert_eq!(w.observe(BoundaryEvent::SilenceTick { now_ms: 60_000 }), Decision::Continue);
    }

    #[test]
    fn closes_after_target_plus_long_silence() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech { start_ms: 0, end_ms: 2_000 });
        // At 4:50 (290s) + speech ending at 4:50, silence until 5:01 (target reached, silence 11s)
        w.observe(BoundaryEvent::Speech { start_ms: 280_000, end_ms: 290_000 });
        assert_eq!(
            w.observe(BoundaryEvent::SilenceTick { now_ms: 301_500 }),
            Decision::CloseClip
        );
    }

    #[test]
    fn does_not_close_at_target_when_speech_is_continuing() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech { start_ms: 0, end_ms: 2_000 });
        // Speech right up to 5:00:000 — no silence gap.
        w.observe(BoundaryEvent::Speech { start_ms: 295_000, end_ms: 300_500 });
        assert_eq!(
            w.observe(BoundaryEvent::SilenceTick { now_ms: 300_700 }),
            Decision::Continue
        );
    }

    #[test]
    fn force_closes_at_max() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech { start_ms: 0, end_ms: 2_000 });
        // Speech goes right up to 6:00 with no gap — we still must close.
        w.observe(BoundaryEvent::Speech { start_ms: 300_000, end_ms: 360_001 });
        // The Speech event itself triggers the cap.
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech { start_ms: 0, end_ms: 2_000 });
        let d = w.observe(BoundaryEvent::Speech { start_ms: 300_000, end_ms: 360_500 });
        assert_eq!(d, Decision::CloseClip);
    }

    #[test]
    fn mute_closes_immediately_if_clip_open() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech { start_ms: 0, end_ms: 1_000 });
        assert_eq!(w.observe(BoundaryEvent::Mute), Decision::CloseClip);
    }

    #[test]
    fn mute_no_op_if_no_clip_open() {
        let mut w = BoundaryWatcher::new(cfg());
        assert_eq!(w.observe(BoundaryEvent::Mute), Decision::Continue);
    }

    #[test]
    fn reset_after_close_starts_fresh_clip_on_next_speech() {
        let mut w = BoundaryWatcher::new(cfg());
        w.observe(BoundaryEvent::Speech { start_ms: 0, end_ms: 2_000 });
        w.observe(BoundaryEvent::Mute);
        w.reset_after_close();
        // Next speech kicks off a new clip; silence at 5:01 closes it again.
        w.observe(BoundaryEvent::Speech { start_ms: 1_000_000, end_ms: 1_002_000 });
        let d = w.observe(BoundaryEvent::SilenceTick { now_ms: 1_303_000 });
        assert_eq!(d, Decision::CloseClip);
    }
}
```

Add `pub mod clip_boundary;` to `backend/actio-core/src/engine/mod.rs`.

Run: `cargo test -p actio-core --lib clip_boundary`
Expected: 7 tests pass.

- [ ] **Step 2: Commit**

```bash
git add backend/actio-core/src/engine/clip_boundary.rs \
        backend/actio-core/src/engine/mod.rs
git commit -m "feat(engine): clip boundary state machine with TDD coverage"
```

---

## Task 6: capture_daemon.rs — long-lived cpal + Silero VAD with broadcast

**Files:**
- Create: `backend/actio-core/src/engine/capture_daemon.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

The capture daemon owns the long-lived cpal stream and the Silero VAD. It produces `CaptureEvent`s on a tokio broadcast channel that the BatchProcessor's segment writer and any LiveStreamingService both subscribe to. Mute is a function call that drops the cpal stream until unmuted.

- [ ] **Step 1: Define events and the daemon handle**

Create `backend/actio-core/src/engine/capture_daemon.rs`:

```rust
//! Long-lived audio capture daemon. Wraps cpal + Silero VAD into a single
//! always-on producer of `CaptureEvent`s. Mute drops the cpal stream;
//! unmute reopens it on the same configured device.
//!
//! Subscribers (the per-clip segment writer + any active LiveStreaming
//! session) receive events through a `tokio::sync::broadcast` channel so
//! late subscribers don't see historical audio.

use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::{info, warn};

use crate::engine::audio_capture::{self, AudioCaptureHandle};
use crate::engine::vad::{self, SpeechSegment, VadConfig};

#[derive(Debug, Clone)]
pub enum CaptureEvent {
    /// One PCM frame; arrives at cpal's callback rate. f32 mono 16 kHz.
    Pcm(Arc<Vec<f32>>),
    /// A finalized VAD speech segment with raw audio samples.
    Speech(Arc<SpeechSegment>),
    /// Capture stopped because the user muted.
    Muted,
    /// Capture resumed.
    Unmuted,
}

pub struct CaptureDaemon {
    inner: Arc<Mutex<Inner>>,
    tx: broadcast::Sender<CaptureEvent>,
}

struct Inner {
    handle: Option<AudioCaptureHandle>,
    device_name: Option<String>,
    vad_cfg: VadConfig,
    muted: bool,
}

impl CaptureDaemon {
    pub fn new(device_name: Option<String>, vad_cfg: VadConfig) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                handle: None,
                device_name,
                vad_cfg,
                muted: false,
            })),
            tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CaptureEvent> {
        self.tx.subscribe()
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let mut g = self.inner.lock().await;
        if g.handle.is_some() {
            return Ok(());
        }
        let (handle, audio_rx) = audio_capture::start_capture(g.device_name.as_deref())?;
        g.handle = Some(handle);
        g.muted = false;

        let tx = self.tx.clone();
        let cfg = g.vad_cfg.clone();
        std::thread::spawn(move || {
            // Bridge cpal callback → VAD → broadcast. VAD owns its own
            // sherpa-onnx Silero session on this thread (sherpa is !Send).
            let mut vad_session = match vad::SileroSession::new(&cfg) {
                Ok(v) => v,
                Err(e) => {
                    warn!(error=%e, "VAD session init failed in capture daemon");
                    return;
                }
            };
            while let Ok(frame) = audio_rx.recv() {
                let _ = tx.send(CaptureEvent::Pcm(Arc::new(frame.clone())));
                if let Some(seg) = vad_session.feed(&frame) {
                    let _ = tx.send(CaptureEvent::Speech(Arc::new(seg)));
                }
            }
        });
        info!("CaptureDaemon started");
        Ok(())
    }

    pub async fn stop(&self) {
        let mut g = self.inner.lock().await;
        g.handle = None;
        info!("CaptureDaemon stopped");
    }

    pub async fn mute(&self) {
        let mut g = self.inner.lock().await;
        if g.muted { return; }
        g.handle = None;
        g.muted = true;
        let _ = self.tx.send(CaptureEvent::Muted);
        info!("CaptureDaemon muted");
    }

    pub async fn unmute(&self) -> anyhow::Result<()> {
        {
            let mut g = self.inner.lock().await;
            if !g.muted { return Ok(()); }
            g.muted = false;
        }
        self.start().await?;
        let _ = self.tx.send(CaptureEvent::Unmuted);
        info!("CaptureDaemon unmuted");
        Ok(())
    }

    pub async fn is_muted(&self) -> bool {
        self.inner.lock().await.muted
    }
}
```

> **Note on `vad::SileroSession`**: this struct may not exist verbatim in the current codebase. Check `engine/vad.rs` and adapt the call (`vad::start_session(&cfg)?` or whatever it actually exports). The contract this code expects: a per-thread VAD wrapper with `feed(&[f32]) -> Option<SpeechSegment>`. If the existing API is callback-based, wrap it in a thin adapter inside this file rather than changing `vad.rs`.

Add `pub mod capture_daemon;` to `backend/actio-core/src/engine/mod.rs`.

- [ ] **Step 2: Compile-check**

```bash
cargo check -p actio-core --tests
```

Expected: clean compile. If `vad.rs` exposes a different API, adapt the bridge thread accordingly — no behaviour change to vad.rs itself.

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/src/engine/capture_daemon.rs \
        backend/actio-core/src/engine/mod.rs
git commit -m "feat(engine): always-on capture daemon with mute toggle and broadcast"
```

> **No unit tests in this task** — the daemon is dominated by cpal/sherpa I/O. End-to-end coverage lives in Task 18 (smoke test).

---

## Task 7: Per-segment WAV writer + manifest writer + clip closure

**Files:**
- Create: `backend/actio-core/src/engine/clip_writer.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`
- Modify: `backend/actio-core/src/domain/types.rs`

`clip_writer.rs` subscribes to `CaptureDaemon`, runs `BoundaryWatcher`, writes per-segment WAVs to `<clips_dir>/<session_id>/<clip_id>/seg_NNNN.wav`, finalizes a `manifest.json`, and inserts an `audio_clips` row.

- [ ] **Step 1: Add `ClipManifest` to `domain/types.rs`**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClipManifestSegment {
    pub id: Uuid,
    pub start_ms: i64,
    pub end_ms: i64,
    pub file: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClipManifest {
    pub clip_id: Uuid,
    pub session_id: Uuid,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub segments: Vec<ClipManifestSegment>,
}
```

- [ ] **Step 2: Write a failing test for manifest round-trip**

Add to `backend/actio-core/src/engine/clip_writer.rs`:

```rust
//! Drains CaptureDaemon events into per-clip on-disk artifacts (segment
//! WAVs + manifest) and inserts the matching `audio_clips` row.
//!
//! Owns the `BoundaryWatcher`. On `Decision::CloseClip` it finalizes the
//! manifest, inserts the row, resets the watcher, and continues.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::types::{ClipManifest, ClipManifestSegment};
use crate::engine::capture_daemon::CaptureEvent;
use crate::engine::clip_boundary::{BoundaryConfig, BoundaryEvent, BoundaryWatcher, Decision};
use crate::engine::vad::SpeechSegment;
use crate::repository::audio_clip;

pub fn write_segment_wav(dir: &Path, name: &str, samples: &[f32]) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&path, spec)?;
    for s in samples {
        w.write_sample(*s)?;
    }
    w.finalize()?;
    Ok(())
}

pub fn write_manifest(dir: &Path, manifest: &ClipManifest) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("manifest.json");
    let body = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&path, body)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn manifest_round_trips_through_disk() {
        let tmp = tempdir().unwrap();
        let m = ClipManifest {
            clip_id: Uuid::nil(),
            session_id: Uuid::nil(),
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![ClipManifestSegment {
                id: Uuid::nil(),
                start_ms: 1_000,
                end_ms: 4_000,
                file: "seg_0001.wav".into(),
            }],
        };
        let path = write_manifest(tmp.path(), &m).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        let back: ClipManifest = serde_json::from_str(&body).unwrap();
        assert_eq!(back.segments.len(), 1);
        assert_eq!(back.segments[0].file, "seg_0001.wav");
    }
}
```

Add `pub mod clip_writer;` to `backend/actio-core/src/engine/mod.rs`.

Run: `cargo test -p actio-core --lib clip_writer`
Expected: PASS.

- [ ] **Step 3: Implement the daemon loop**

Append:

```rust
pub struct ClipWriterConfig {
    pub clips_dir: PathBuf,
    pub boundary: BoundaryConfig,
}

pub async fn run_clip_writer_loop(
    pool: sqlx::SqlitePool,
    session_id: Uuid,
    cfg: ClipWriterConfig,
    mut events: broadcast::Receiver<CaptureEvent>,
) {
    let mut watcher = BoundaryWatcher::new(cfg.boundary);
    let mut current_clip_id: Option<Uuid> = None;
    let mut current_dir: Option<PathBuf> = None;
    let mut segments: Vec<ClipManifestSegment> = Vec::new();
    let mut clip_started_ms: Option<i64> = None;
    let mut next_seg_idx: usize = 0;

    loop {
        let ev = match events.recv().await {
            Ok(ev) => ev,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!(skipped = n, "ClipWriter lagged; some PCM dropped");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        };
        let boundary_event = match &ev {
            CaptureEvent::Speech(seg) => Some(BoundaryEvent::Speech {
                start_ms: seg.start_ms,
                end_ms: seg.end_ms,
            }),
            CaptureEvent::Pcm(_) => None,
            CaptureEvent::Muted => Some(BoundaryEvent::Mute),
            CaptureEvent::Unmuted => None,
        };

        if let CaptureEvent::Speech(seg) = &ev {
            // Lazily open a new clip on first speech.
            if current_clip_id.is_none() {
                let clip_id = Uuid::new_v4();
                let dir = cfg.clips_dir.join(session_id.to_string()).join(clip_id.to_string());
                if let Err(e) = std::fs::create_dir_all(&dir) {
                    warn!(error=%e, "ClipWriter could not create clip dir");
                    continue;
                }
                current_clip_id = Some(clip_id);
                current_dir = Some(dir);
                clip_started_ms = Some(seg.start_ms);
                next_seg_idx = 0;
                segments.clear();
            }
            let dir = current_dir.as_ref().unwrap().clone();
            next_seg_idx += 1;
            let name = format!("seg_{:04}.wav", next_seg_idx);
            if let Err(e) = write_segment_wav(&dir, &name, &seg.samples) {
                warn!(error=%e, "Failed to write segment WAV");
                continue;
            }
            segments.push(ClipManifestSegment {
                id: seg.id,
                start_ms: seg.start_ms,
                end_ms: seg.end_ms,
                file: name,
            });
        }

        if let Some(be) = boundary_event {
            if matches!(watcher.observe(be), Decision::CloseClip) {
                if let (Some(clip_id), Some(dir), Some(started)) =
                    (current_clip_id, current_dir.clone(), clip_started_ms)
                {
                    let ended_at_ms = segments.last().map(|s| s.end_ms).unwrap_or(started);
                    let manifest = ClipManifest {
                        clip_id,
                        session_id,
                        started_at_ms: started,
                        ended_at_ms,
                        segments: std::mem::take(&mut segments),
                    };
                    let manifest_path = match write_manifest(&dir, &manifest) {
                        Ok(p) => p,
                        Err(e) => {
                            warn!(error=%e, "Failed to write manifest");
                            current_clip_id = None;
                            current_dir = None;
                            clip_started_ms = None;
                            watcher.reset_after_close();
                            continue;
                        }
                    };
                    if let Err(e) = audio_clip::insert_pending(
                        &pool,
                        session_id,
                        manifest.started_at_ms,
                        manifest.ended_at_ms,
                        manifest.segments.len() as i64,
                        manifest_path.to_string_lossy().as_ref(),
                    )
                    .await
                    {
                        warn!(error=%e, "Failed to insert audio_clips row");
                    } else {
                        info!(%clip_id, "audio clip closed and queued");
                    }
                }
                current_clip_id = None;
                current_dir = None;
                clip_started_ms = None;
                watcher.reset_after_close();
            }
        }
    }
}
```

> The `SpeechSegment` struct must expose `id: Uuid`, `start_ms: i64`, `end_ms: i64`, and `samples: Vec<f32>`. If the existing `vad::SpeechSegment` differs, add a `From` impl in `vad.rs` rather than reaching into the daemon.

- [ ] **Step 4: Compile**

```bash
cargo check -p actio-core --tests
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/clip_writer.rs \
        backend/actio-core/src/engine/mod.rs \
        backend/actio-core/src/domain/types.rs
git commit -m "feat(engine): clip writer — segment WAVs, manifest, audio_clips row"
```

---

## Task 8: BatchProcessor — ASR + transcript persistence (no clustering yet)

**Files:**
- Create: `backend/actio-core/src/engine/batch_processor.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

This task lands a single-worker batch processor that loads the archive ASR cold, transcribes each segment WAV in the manifest, writes `transcripts` rows tied to the existing `audio_segments` rows. Clustering and provisional speakers come in Task 9.

- [ ] **Step 1: Write a failing integration test**

Add the test module to a new `backend/actio-core/src/engine/batch_processor.rs`:

```rust
//! Single-worker batch processor over the audio_clips queue.
//!
//! Pulls one pending clip, loads its manifest, cold-loads the archive ASR
//! model in a `spawn_blocking` worker, transcribes each segment WAV, writes
//! transcripts/audio_segments rows tied to the clip, and marks the clip
//! processed. Clustering + speaker assignment is added in Task 9.

use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::domain::types::{AudioClip, ClipManifest, ClipManifestSegment};
use crate::repository::{audio_clip, segment, transcript};

#[derive(Debug, Clone)]
pub struct ArchiveTranscript {
    pub segment_id: Uuid,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Trait abstraction over the archive ASR for testability. Production impl
/// wraps sherpa-onnx; tests inject a deterministic stub.
pub trait ArchiveAsr: Send + Sync {
    fn transcribe_clip(
        &self,
        manifest: &ClipManifest,
        audio_dir: &std::path::Path,
    ) -> anyhow::Result<Vec<ArchiveTranscript>>;
}

pub async fn process_clip<A: ArchiveAsr>(
    pool: &sqlx::SqlitePool,
    asr: &A,
    clip: &AudioClip,
) -> anyhow::Result<()> {
    let manifest_body = std::fs::read_to_string(&clip.manifest_path)?;
    let manifest: ClipManifest = serde_json::from_str(&manifest_body)?;
    let audio_dir = std::path::Path::new(&clip.manifest_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    if manifest.segments.is_empty() {
        audio_clip::mark_empty(pool, clip.id).await?;
        return Ok(());
    }

    // 1) Persist audio_segments rows tied to this clip.
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

    // 2) Run ASR (cold model load happens inside the impl).
    let transcripts = asr.transcribe_clip(&manifest, &audio_dir)?;

    // 3) Persist transcripts.
    for t in &transcripts {
        transcript::insert_finalized(
            pool,
            clip.session_id,
            t.segment_id,
            t.start_ms,
            t.end_ms,
            &t.text,
        )
        .await?;
    }

    audio_clip::mark_processed(pool, clip.id, None).await?;
    info!(clip_id = %clip.id, count = transcripts.len(), "clip processed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::clip_writer::write_manifest;
    use crate::repository::db::test_helpers::test_pool;
    use crate::repository::session;
    use tempfile::tempdir;

    struct StubAsr;
    impl ArchiveAsr for StubAsr {
        fn transcribe_clip(
            &self,
            manifest: &ClipManifest,
            _audio_dir: &std::path::Path,
        ) -> anyhow::Result<Vec<ArchiveTranscript>> {
            Ok(manifest.segments.iter().map(|s| ArchiveTranscript {
                segment_id: s.id,
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: format!("seg{}", s.start_ms),
            }).collect())
        }
    }

    #[tokio::test]
    async fn process_clip_writes_segments_and_transcripts_and_marks_processed() {
        let pool = test_pool().await;
        let session_id = session::create_default_session(&pool).await.unwrap();
        let tmp = tempdir().unwrap();
        let manifest = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![
                ClipManifestSegment { id: Uuid::new_v4(), start_ms: 1_000, end_ms: 3_000, file: "seg_0001.wav".into() },
                ClipManifestSegment { id: Uuid::new_v4(), start_ms: 4_000, end_ms: 6_000, file: "seg_0002.wav".into() },
            ],
        };
        let manifest_path = write_manifest(tmp.path(), &manifest).unwrap();
        let clip_id = audio_clip::insert_pending(
            &pool, session_id, 0, 300_000, 2,
            manifest_path.to_string_lossy().as_ref(),
        ).await.unwrap();

        // Claim manually (process_clip expects an AudioClip).
        let claimed = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(claimed.id, clip_id);

        process_clip(&pool, &StubAsr, &claimed).await.unwrap();

        let after = audio_clip::get_by_id(&pool, clip_id).await.unwrap().unwrap();
        assert_eq!(after.status, "processed");

        let transcripts = transcript::list_for_session(&pool, session_id).await.unwrap();
        assert_eq!(transcripts.len(), 2);
    }
}
```

Add `pub mod batch_processor;` to `backend/actio-core/src/engine/mod.rs`.

- [ ] **Step 2: Add the two repository helpers the test relies on**

In `backend/actio-core/src/repository/segment.rs`, add:

```rust
/// Insert (or no-op-update) the audio_segments row for a clip-attributed
/// segment. Used by the batch processor before transcripts/embeddings land.
pub async fn upsert_segment_for_clip(
    pool: &SqlitePool,
    id: Uuid,
    session_id: Uuid,
    clip_id: Uuid,
    start_ms: i64,
    end_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO audio_segments (id, session_id, clip_id, start_ms, end_ms)
           VALUES (?1, ?2, ?3, ?4, ?5)
           ON CONFLICT(id) DO UPDATE SET clip_id = excluded.clip_id"#,
    )
    .bind(id.to_string())
    .bind(session_id.to_string())
    .bind(clip_id.to_string())
    .bind(start_ms)
    .bind(end_ms)
    .execute(pool)
    .await?;
    Ok(())
}
```

In `backend/actio-core/src/repository/transcript.rs`, ensure `insert_finalized` exists with the signature `(&Pool, session_id, segment_id, start_ms, end_ms, text) -> Result<()>`. If it doesn't, add it now mirroring the existing transcript insert site.

- [ ] **Step 3: Run the integration test**

```bash
cargo test -p actio-core --lib batch_processor
```

Expected: PASS.

- [ ] **Step 4: Add the production sherpa-based `ArchiveAsr` impl**

Append to `batch_processor.rs`:

```rust
pub struct SherpaArchiveAsr {
    pub model_id: String,
    pub model_paths: crate::engine::model_manager::ModelPaths,
}

impl ArchiveAsr for SherpaArchiveAsr {
    fn transcribe_clip(
        &self,
        manifest: &ClipManifest,
        audio_dir: &std::path::Path,
    ) -> anyhow::Result<Vec<ArchiveTranscript>> {
        // Cold-load the recognizer in this thread (sherpa is !Send).
        let mut recognizer = crate::engine::asr::cold_recognizer(&self.model_id, &self.model_paths)?;
        let mut out = Vec::with_capacity(manifest.segments.len());
        for seg in &manifest.segments {
            let path = audio_dir.join(&seg.file);
            let samples = read_wav_f32_mono_16k(&path)?;
            let text = recognizer.decode_full(&samples)?;
            out.push(ArchiveTranscript {
                segment_id: seg.id,
                start_ms: seg.start_ms,
                end_ms: seg.end_ms,
                text,
            });
        }
        Ok(out)
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
        Ok(reader.samples::<i32>().filter_map(|s| s.ok())
            .map(|x| x as f32 / max).collect())
    }
}
```

> Add `cold_recognizer(model_id, paths) -> Result<impl Decoder>` and a `decode_full(&[f32]) -> Result<String>` to `engine/asr.rs`. The shape mirrors the existing streaming recognizer constructor — a non-streaming variant that takes the whole sample buffer at once. If the existing ASR module only exposes streaming, write a thin wrapper that feeds the full buffer in one shot and finalizes.

- [ ] **Step 5: Compile**

```bash
cargo check -p actio-core --tests
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/engine/batch_processor.rs \
        backend/actio-core/src/engine/mod.rs \
        backend/actio-core/src/engine/asr.rs \
        backend/actio-core/src/repository/segment.rs \
        backend/actio-core/src/repository/transcript.rs
git commit -m "feat(engine): batch processor — clip→ASR→transcripts (no clustering yet)"
```

---

## Task 9: BatchProcessor — embedding + clustering + speaker matching + auto-provisional

**Files:**
- Modify: `backend/actio-core/src/engine/batch_processor.rs`
- Modify: `backend/actio-core/src/repository/speaker.rs`

- [ ] **Step 1: Write failing tests for clustering integration**

Append to the test module of `batch_processor.rs`:

```rust
    struct StubEmbedder { fixed: Vec<Vec<f32>> }
    impl SegmentEmbedder for StubEmbedder {
        fn embed_segments(
            &self,
            manifest: &ClipManifest,
            _audio_dir: &std::path::Path,
        ) -> anyhow::Result<Vec<(Uuid, Vec<f32>)>> {
            Ok(manifest.segments.iter().enumerate()
                .map(|(i, s)| (s.id, self.fixed[i].clone())).collect())
        }
        fn dimension(&self) -> i64 { 2 }
    }

    #[tokio::test]
    async fn cluster_and_provisional_speakers_get_persisted() {
        let pool = test_pool().await;
        let session_id = session::create_default_session(&pool).await.unwrap();
        let tmp = tempdir().unwrap();
        let manifest = ClipManifest {
            clip_id: Uuid::new_v4(),
            session_id,
            started_at_ms: 0,
            ended_at_ms: 300_000,
            segments: vec![
                ClipManifestSegment { id: Uuid::new_v4(), start_ms: 1_000, end_ms: 3_000, file: "1.wav".into() },
                ClipManifestSegment { id: Uuid::new_v4(), start_ms: 4_000, end_ms: 6_000, file: "2.wav".into() },
                ClipManifestSegment { id: Uuid::new_v4(), start_ms: 7_000, end_ms: 9_000, file: "3.wav".into() },
            ],
        };
        let manifest_path = write_manifest(tmp.path(), &manifest).unwrap();
        audio_clip::insert_pending(&pool, session_id, 0, 300_000, 3,
            manifest_path.to_string_lossy().as_ref()).await.unwrap();
        let claimed = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();

        let embedder = StubEmbedder { fixed: vec![
            vec![1.0, 0.0],
            vec![0.99, 0.14],
            vec![0.0, 1.0],
        ] };
        let cfg = ClusteringConfig { cosine_threshold: 0.4, min_segments_per_cluster: 1 };

        process_clip_with_clustering(&pool, &StubAsr, &embedder, &claimed, &cfg).await.unwrap();

        // Two clusters → two new provisional speakers.
        let speakers = crate::repository::speaker::list_provisional(&pool).await.unwrap();
        assert_eq!(speakers.len(), 2);

        let segs = crate::repository::segment::list_for_clip(&pool, claimed.id).await.unwrap();
        assert_eq!(segs.len(), 3);
        // First two segments share clip_local_speaker_idx; third differs.
        assert_eq!(segs[0].clip_local_speaker_idx, segs[1].clip_local_speaker_idx);
        assert_ne!(segs[0].clip_local_speaker_idx, segs[2].clip_local_speaker_idx);
        // All three segments got speaker_id assigned.
        assert!(segs.iter().all(|s| s.speaker_id.is_some()));
    }

    #[tokio::test]
    async fn second_clip_with_same_centroid_links_to_first_provisional() {
        let pool = test_pool().await;
        let session_id = session::create_default_session(&pool).await.unwrap();
        let cfg = ClusteringConfig { cosine_threshold: 0.4, min_segments_per_cluster: 1 };

        // Clip 1
        let tmp = tempdir().unwrap();
        let m1 = ClipManifest {
            clip_id: Uuid::new_v4(), session_id, started_at_ms: 0, ended_at_ms: 300_000,
            segments: vec![ClipManifestSegment { id: Uuid::new_v4(), start_ms: 0, end_ms: 1_000, file: "1.wav".into() }],
        };
        let p1 = write_manifest(tmp.path(), &m1).unwrap();
        audio_clip::insert_pending(&pool, session_id, 0, 300_000, 1, p1.to_string_lossy().as_ref()).await.unwrap();
        let c1 = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
        let e1 = StubEmbedder { fixed: vec![vec![1.0, 0.0]] };
        process_clip_with_clustering(&pool, &StubAsr, &e1, &c1, &cfg).await.unwrap();

        // Clip 2 with same centroid
        let tmp2 = tempdir().unwrap();
        let m2 = ClipManifest {
            clip_id: Uuid::new_v4(), session_id, started_at_ms: 300_000, ended_at_ms: 600_000,
            segments: vec![ClipManifestSegment { id: Uuid::new_v4(), start_ms: 300_000, end_ms: 301_000, file: "1.wav".into() }],
        };
        let p2 = write_manifest(tmp2.path(), &m2).unwrap();
        audio_clip::insert_pending(&pool, session_id, 300_000, 600_000, 1, p2.to_string_lossy().as_ref()).await.unwrap();
        let c2 = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
        let e2 = StubEmbedder { fixed: vec![vec![0.99, 0.14]] };
        process_clip_with_clustering(&pool, &StubAsr, &e2, &c2, &cfg).await.unwrap();

        let speakers = crate::repository::speaker::list_provisional(&pool).await.unwrap();
        assert_eq!(speakers.len(), 1, "second clip should reuse the first clip's provisional row");
    }
```

Run: `cargo test -p actio-core --lib batch_processor`
Expected: FAIL — `process_clip_with_clustering`, `SegmentEmbedder`, `ClusteringConfig`, `list_provisional`, `list_for_clip` don't exist.

- [ ] **Step 2: Add `SegmentEmbedder` trait and clustering pipeline**

Append to `batch_processor.rs`:

```rust
pub trait SegmentEmbedder: Send + Sync {
    fn embed_segments(
        &self,
        manifest: &ClipManifest,
        audio_dir: &std::path::Path,
    ) -> anyhow::Result<Vec<(Uuid, Vec<f32>)>>;
    fn dimension(&self) -> i64;
}

pub struct ClusteringConfig {
    pub cosine_threshold: f32,
    pub min_segments_per_cluster: usize,
}

pub async fn process_clip_with_clustering<A: ArchiveAsr, E: SegmentEmbedder>(
    pool: &sqlx::SqlitePool,
    asr: &A,
    embedder: &E,
    clip: &AudioClip,
    cfg: &ClusteringConfig,
) -> anyhow::Result<()> {
    let manifest_body = std::fs::read_to_string(&clip.manifest_path)?;
    let manifest: ClipManifest = serde_json::from_str(&manifest_body)?;
    let audio_dir = std::path::Path::new(&clip.manifest_path)
        .parent().map(|p| p.to_path_buf()).unwrap_or_default();

    if manifest.segments.is_empty() {
        audio_clip::mark_empty(pool, clip.id).await?;
        return Ok(());
    }

    // 1) Persist segments + embeddings.
    let embeddings = embedder.embed_segments(&manifest, &audio_dir)?;
    for seg in &manifest.segments {
        segment::upsert_segment_for_clip(pool, seg.id, clip.session_id, clip.id, seg.start_ms, seg.end_ms).await?;
    }
    for (id, emb) in &embeddings {
        segment::set_embedding(pool, *id, emb, embedder.dimension()).await?;
    }

    // 2) ASR.
    let transcripts = asr.transcribe_clip(&manifest, &audio_dir)?;
    for t in &transcripts {
        transcript::insert_finalized(pool, clip.session_id, t.segment_id, t.start_ms, t.end_ms, &t.text).await?;
    }

    // 3) Cluster.
    let assignments = crate::engine::cluster::ahc(&embeddings, cfg.cosine_threshold);
    let mut clusters: std::collections::BTreeMap<usize, Vec<(Uuid, &[f32])>> = Default::default();
    for (i, a) in assignments.iter().enumerate() {
        clusters.entry(a.cluster_idx).or_default().push((a.segment_id, embeddings[i].1.as_slice()));
    }

    // 4) For each cluster, compute centroid, match against speakers, assign or create provisional.
    let tenant_id = uuid::Uuid::nil(); // single-tenant default
    for (cluster_idx, members) in clusters {
        if members.len() < cfg.min_segments_per_cluster { continue; }
        let centroid = mean_unit(&members.iter().map(|(_, e)| e.to_vec()).collect::<Vec<_>>());
        let speaker_id = match crate::repository::speaker::find_match_by_embedding(
            pool, &centroid, embedder.dimension(), tenant_id,
        ).await? {
            Some(id) => {
                crate::repository::speaker::touch_provisional_match(pool, id).await?;
                id
            }
            None => {
                let new_id = uuid::Uuid::new_v4();
                let now = chrono::Utc::now();
                let display_name = format!("Unknown {}", now.format("%Y-%m-%d %H:%M"));
                crate::repository::speaker::insert_provisional(
                    pool, new_id, tenant_id, &display_name,
                ).await?;
                new_id
            }
        };
        for (seg_id, _) in members {
            segment::assign_speaker_and_local_idx(pool, seg_id, speaker_id, cluster_idx as i64).await?;
        }
    }

    audio_clip::mark_processed(pool, clip.id, None).await?;
    Ok(())
}

fn mean_unit(vs: &[Vec<f32>]) -> Vec<f32> {
    if vs.is_empty() { return Vec::new(); }
    let dim = vs[0].len();
    let mut acc = vec![0.0_f32; dim];
    for v in vs { for (i, x) in v.iter().enumerate() { acc[i] += x; } }
    let n = vs.len() as f32;
    for x in acc.iter_mut() { *x /= n; }
    let norm = acc.iter().map(|x| x*x).sum::<f32>().sqrt().max(1e-8);
    for x in acc.iter_mut() { *x /= norm; }
    acc
}
```

- [ ] **Step 3: Add the speaker repository helpers**

Append to `backend/actio-core/src/repository/speaker.rs`:

```rust
pub async fn insert_provisional(
    pool: &sqlx::SqlitePool,
    id: Uuid,
    tenant_id: Uuid,
    display_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO speakers (id, tenant_id, display_name, status, kind, provisional_last_matched_at)
           VALUES (?1, ?2, ?3, 'active', 'provisional',
                   strftime('%Y-%m-%dT%H:%M:%fZ','now'))"#,
    )
    .bind(id.to_string())
    .bind(tenant_id.to_string())
    .bind(display_name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn touch_provisional_match(pool: &sqlx::SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE speakers
           SET provisional_last_matched_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1 AND kind = 'provisional'"#,
    )
    .bind(id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_provisional(pool: &sqlx::SqlitePool) -> Result<Vec<ProvisionalSpeaker>, sqlx::Error> {
    let rows = sqlx::query_as::<_, ProvisionalSpeakerRow>(
        r#"SELECT id, tenant_id, display_name, provisional_last_matched_at
           FROM speakers WHERE kind = 'provisional' AND status = 'active'
           ORDER BY provisional_last_matched_at DESC"#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| ProvisionalSpeaker {
        id: Uuid::parse_str(&r.id).unwrap_or_default(),
        tenant_id: Uuid::parse_str(&r.tenant_id).unwrap_or_default(),
        display_name: r.display_name,
        last_matched_at: r.provisional_last_matched_at,
    }).collect())
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct ProvisionalSpeakerRow {
    pub id: String,
    pub tenant_id: String,
    pub display_name: String,
    pub provisional_last_matched_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProvisionalSpeaker {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub display_name: String,
    pub last_matched_at: Option<String>,
}

/// Match a centroid against all speakers' aggregate embeddings (their
/// audio_segments rows). Returns the speaker_id whose mean embedding
/// is the closest match above `confirm_threshold`, otherwise None.
pub async fn find_match_by_embedding(
    pool: &sqlx::SqlitePool,
    centroid: &[f32],
    dim: i64,
    tenant_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    use crate::engine::diarization::cosine_similarity;
    let rows = sqlx::query_as::<_, (String, Vec<u8>)>(
        r#"SELECT sp.id, seg.embedding
           FROM speakers sp
           JOIN audio_segments seg ON seg.speaker_id = sp.id
           WHERE sp.tenant_id = ?1
             AND sp.status = 'active'
             AND seg.embedding IS NOT NULL
             AND seg.embedding_dim = ?2"#,
    )
    .bind(tenant_id.to_string())
    .bind(dim)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() { return Ok(None); }

    let mut sums: std::collections::BTreeMap<String, (Vec<f32>, usize)> = Default::default();
    for (id, blob) in rows {
        let v = blob.chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect::<Vec<f32>>();
        if v.len() != dim as usize { continue; }
        let entry = sums.entry(id).or_insert_with(|| (vec![0.0; dim as usize], 0));
        for (i, x) in v.iter().enumerate() { entry.0[i] += x; }
        entry.1 += 1;
    }
    let confirm_threshold: f32 = 0.55; // mirrors AudioSettings default
    let mut best: Option<(String, f32)> = None;
    for (id, (sum, n)) in sums {
        if n == 0 { continue; }
        let mean: Vec<f32> = sum.iter().map(|x| x / n as f32).collect();
        let norm = mean.iter().map(|x| x*x).sum::<f32>().sqrt().max(1e-8);
        let unit: Vec<f32> = mean.iter().map(|x| x / norm).collect();
        let sim = cosine_similarity(&unit, centroid) as f32;
        if best.as_ref().map_or(true, |(_, b)| sim > *b) {
            best = Some((id, sim));
        }
    }
    Ok(best.and_then(|(id, sim)| if sim >= confirm_threshold {
        Uuid::parse_str(&id).ok()
    } else { None }))
}
```

> The exact embedding storage format (currently `BLOB` of little-endian f32) and the existing `cosine_similarity` signature should be checked against `engine/diarization.rs`. If embeddings are stored differently in this repo, adapt the `chunks_exact(4)` decode accordingly.

In `repository/segment.rs`, add:

```rust
pub async fn set_embedding(
    pool: &sqlx::SqlitePool,
    id: Uuid,
    embedding: &[f32],
    dim: i64,
) -> Result<(), sqlx::Error> {
    let mut blob = Vec::with_capacity(embedding.len() * 4);
    for x in embedding { blob.extend_from_slice(&x.to_le_bytes()); }
    sqlx::query(
        r#"UPDATE audio_segments SET embedding = ?2, embedding_dim = ?3 WHERE id = ?1"#,
    )
    .bind(id.to_string()).bind(blob).bind(dim).execute(pool).await?;
    Ok(())
}

pub async fn assign_speaker_and_local_idx(
    pool: &sqlx::SqlitePool,
    seg_id: Uuid,
    speaker_id: Uuid,
    local_idx: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE audio_segments
           SET speaker_id = ?2, clip_local_speaker_idx = ?3
           WHERE id = ?1"#,
    )
    .bind(seg_id.to_string()).bind(speaker_id.to_string()).bind(local_idx)
    .execute(pool).await?;
    Ok(())
}

pub async fn list_for_clip(pool: &sqlx::SqlitePool, clip_id: Uuid) -> Result<Vec<SegmentRow>, sqlx::Error> {
    sqlx::query_as::<_, SegmentRow>(
        r#"SELECT id, session_id, clip_id, speaker_id, start_ms, end_ms, clip_local_speaker_idx
           FROM audio_segments WHERE clip_id = ?1 ORDER BY start_ms"#,
    )
    .bind(clip_id.to_string()).fetch_all(pool).await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SegmentRow {
    pub id: String,
    pub session_id: String,
    pub clip_id: Option<String>,
    pub speaker_id: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub clip_local_speaker_idx: Option<i64>,
}
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p actio-core --lib batch_processor
```

Expected: PASS — both new tests plus the Task 8 test.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/batch_processor.rs \
        backend/actio-core/src/repository/speaker.rs \
        backend/actio-core/src/repository/segment.rs
git commit -m "feat(engine): clip-level clustering + auto-provisional speakers"
```

---

## Task 10: Refactor window_extractor — driven by post-clip hook

**Files:**
- Modify: `backend/actio-core/src/engine/window_extractor.rs`
- Modify: `backend/actio-core/src/engine/batch_processor.rs`

The window extractor stops scheduling its own time-windows. Instead, the BatchProcessor calls `extract_for_clip(&pool, &state, clip_id)` after `mark_processed`, which builds the prompt from the just-written transcripts and runs the LLM as before.

- [ ] **Step 1: Write a failing test**

Add to `window_extractor.rs` test module:

```rust
#[tokio::test]
async fn extract_for_clip_writes_high_confidence_reminders_with_source_window_id() {
    let pool = test_pool().await;
    let session_id = session::create_default_session(&pool).await.unwrap();
    // Insert a clip with two transcripts.
    let clip_id = audio_clip::insert_pending(&pool, session_id, 0, 300_000, 1, "/tmp/x.json").await.unwrap();
    let _ = audio_clip::claim_next_pending(&pool).await.unwrap();
    audio_clip::mark_processed(&pool, clip_id, None).await.unwrap();
    // Seed a transcript so the prompt is non-empty.
    let seg_id = uuid::Uuid::new_v4();
    crate::repository::segment::upsert_segment_for_clip(&pool, seg_id, session_id, clip_id, 1_000, 4_000).await.unwrap();
    crate::repository::transcript::insert_finalized(&pool, session_id, seg_id, 1_000, 4_000, "Buy milk by Friday.").await.unwrap();

    let stub_router = StubRouter::with_items(vec![StubItem {
        description: "Buy milk".into(),
        confidence: "high".into(),
        ..Default::default()
    }]);

    extract_for_clip(&pool, &stub_router, clip_id).await.unwrap();

    let reminders = crate::repository::reminder::list_for_session(&pool, session_id).await.unwrap();
    assert_eq!(reminders.len(), 1);
    assert_eq!(reminders[0].source_window_id, Some(clip_id));
    assert_eq!(reminders[0].status, "open");
}
```

> If `StubRouter` / `StubItem` don't exist, write them in this test module — minimal fakes mirroring `LlmRouter::generate_action_items_with_refs` signature.

Run: `cargo test -p actio-core --lib extract_for_clip`
Expected: FAIL — `extract_for_clip` doesn't exist.

- [ ] **Step 2: Implement `extract_for_clip`**

Refactor `window_extractor.rs`:

1. Keep the existing prompt-building helpers (`format_window_input`, the confidence gating logic, the `MIN_EXTRACTABLE_CHARS` short-circuit, and reminder insertion).
2. Replace `tick_once` and `run_extraction_loop` with one `pub async fn extract_for_clip(pool, router, clip_id)`:
   - Read transcripts WHERE clip_id matches via the new `transcript::list_for_clip` helper.
   - Read segments + speakers via the existing helpers, joined on the same `clip_id`.
   - Build the prompt input the same way the legacy `tick_once` did.
   - Short-circuit `empty` if `text.len() < MIN_EXTRACTABLE_CHARS`.
   - Call `router.generate_action_items_with_refs(...)`.
   - On `LlmRouterError::Disabled`, log and return Ok — the clip is already `processed`, the user can re-run extraction by toggling the LLM and we accept that the action items for this clip won't appear unless we re-process. (See follow-up note in Task 17.)
   - On other errors, log and return; do not flip the clip back to failed (it's already processed).
   - Insert reminders with `source_window_id = clip_id` and the same confidence gates (`high` → `open`, `medium` → `pending`, else dropped).

3. Delete `run_extraction_loop`, `tick_once`, `schedule_windows_for_active_sessions`, `claim_next_pending` (the function — `audio_clip::claim_next_pending` replaces it), and the `MAX_WINDOWS_PER_TICK` / `SAFETY_MARGIN_MS` constants.

4. Add a thin helper to `repository/transcript.rs`:

```rust
pub async fn list_for_clip(pool: &sqlx::SqlitePool, clip_id: Uuid) -> Result<Vec<TranscriptRow>, sqlx::Error> {
    sqlx::query_as::<_, TranscriptRow>(
        r#"SELECT t.* FROM transcripts t
           JOIN audio_segments s ON s.id = t.segment_id
           WHERE s.clip_id = ?1
           ORDER BY t.start_ms"#,
    )
    .bind(clip_id.to_string()).fetch_all(pool).await
}
```

5. Wire BatchProcessor's `process_clip_with_clustering` to call `extract_for_clip` after `mark_processed`. Make the call best-effort (log on error, never fail the clip):

```rust
    // 5) Action-item extraction.
    if let Err(e) = crate::engine::window_extractor::extract_for_clip(pool, router, clip.id).await {
        tracing::warn!(error=%e, clip_id=%clip.id, "post-clip extraction failed");
    }
```

The `process_clip_with_clustering` signature gains a `router: &LlmRouter` parameter; tests should pass a stub router that returns `LlmRouterError::Disabled` to keep the existing test assertions valid (no reminders inserted).

- [ ] **Step 3: Run the tests**

```bash
cargo test -p actio-core --lib window_extractor
cargo test -p actio-core --lib batch_processor
```

Expected: both green.

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/window_extractor.rs \
        backend/actio-core/src/engine/batch_processor.rs \
        backend/actio-core/src/repository/transcript.rs
git commit -m "refactor(extractor): drive action items from post-clip hook"
```

---

## Task 11: live_streaming.rs — extract from inference_pipeline, no DB writes

**Files:**
- Create: `backend/actio-core/src/engine/live_streaming.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`
- Modify: `backend/actio-core/src/engine/inference_pipeline.rs` (shrink to a re-export shim)

- [ ] **Step 1: Survey the existing pipeline**

Open `engine/inference_pipeline.rs` and identify the parts that:
1. Subscribe to capture audio (now provided by `CaptureDaemon::subscribe`).
2. Run per-segment ASR (existing streaming path).
3. Run the embedder + `ContinuityState`.
4. Broadcast `transcript` and `speaker_resolved` frames on `/ws`.
5. Persist anything to the DB (transcripts, audio_segments).

Identify (5) — those code paths must be **deleted** in the live path. Persistence is now batch-only.

- [ ] **Step 2: Create `live_streaming.rs`**

New file with this shape:

```rust
//! On-demand live streaming service. Spun up while dictation or
//! translation is active. Subscribes to the CaptureDaemon, runs per-segment
//! streaming ASR + the existing continuity state machine, and broadcasts
//! `transcript` and `speaker_resolved` frames on /ws. **Writes nothing to
//! the database** — the persisted archive comes from the batch processor.

use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::info;
use uuid::Uuid;

use crate::engine::capture_daemon::{CaptureDaemon, CaptureEvent};
use crate::engine::continuity::ContinuityState;
use crate::engine::inference_pipeline::SpeakerIdConfig;
use crate::engine::vad::SpeechSegment;

pub struct LiveStreamingService {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

impl LiveStreamingService {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(Inner { cancel: None })) }
    }

    pub async fn is_running(&self) -> bool {
        self.inner.lock().await.cancel.is_some()
    }

    pub async fn start(
        &self,
        session_id: Uuid,
        capture: Arc<CaptureDaemon>,
        live_asr_model: String,
        speaker_id_cfg: SpeakerIdConfig,
        ws_tx: broadcast::Sender<crate::api::ws::OutgoingFrame>,
    ) -> anyhow::Result<()> {
        let mut g = self.inner.lock().await;
        if g.cancel.is_some() { return Ok(()); }
        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        g.cancel = Some(cancel_tx);
        drop(g);

        let mut events = capture.subscribe();
        tokio::spawn(async move {
            // Spin up streaming recognizer and continuity in a spawn_blocking
            // worker (sherpa is !Send). Bridge VAD segments → ASR text →
            // ws_tx. Same shape as today's InferencePipeline minus DB writes.
            let mut continuity = ContinuityState::default();
            let _ = run_streaming_loop(
                session_id, &mut continuity, &live_asr_model, speaker_id_cfg, &mut events, &ws_tx, cancel_rx,
            ).await;
            info!("LiveStreamingService stopped");
        });
        Ok(())
    }

    pub async fn stop(&self) {
        let mut g = self.inner.lock().await;
        if let Some(tx) = g.cancel.take() { let _ = tx.send(()); }
    }
}

async fn run_streaming_loop(
    _session_id: Uuid,
    _continuity: &mut ContinuityState,
    _live_asr_model: &str,
    _cfg: SpeakerIdConfig,
    _events: &mut broadcast::Receiver<CaptureEvent>,
    _ws_tx: &broadcast::Sender<crate::api::ws::OutgoingFrame>,
    _cancel: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    // TODO(impl): port the per-segment ASR + speaker identify flow from
    // inference_pipeline.rs::start_session, MINUS any DB write. Return on cancel.
    Ok(())
}
```

> The `run_streaming_loop` body is a port of the existing `InferencePipeline::start_session` event loop — copy the `tokio::spawn_blocking` worker structure, the `crossbeam_channel` for ASR results, and the per-segment identify path verbatim, but *remove* every call site that writes to the database. The continuity state machine and the WS broadcast stay.

- [ ] **Step 3: Shrink `inference_pipeline.rs`**

Replace the file with:

```rust
//! Compatibility shim. The streaming pipeline's runtime moved to
//! `live_streaming.rs`. This module now only re-exports the small types
//! that other crates still reference (`SpeakerIdConfig`).

pub use crate::engine::live_streaming::LiveStreamingService;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerIdConfig {
    pub confirm_threshold: f32,
    pub tentative_threshold: f32,
    pub min_duration_ms: u32,
    pub continuity_window_ms: u32,
}

impl Default for SpeakerIdConfig {
    fn default() -> Self {
        Self { confirm_threshold: 0.55, tentative_threshold: 0.40, min_duration_ms: 1500, continuity_window_ms: 15_000 }
    }
}
```

> All the cpal+VAD+ASR scaffolding the old file held now lives between `capture_daemon.rs`, `live_streaming.rs`, and `batch_processor.rs`. Anything left over (e.g. `quality_score_for_voiceprint_candidate` helpers used by other modules) should be relocated to its actual call-site module rather than left as a re-export.

- [ ] **Step 4: Compile**

```bash
cargo check -p actio-core --tests
```

Resolve breaks at call sites — `state.rs`, `api/session.rs`, anything that takes an `InferencePipeline` directly. Replace with `Arc<CaptureDaemon>` + `Arc<LiveStreamingService>` references.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/live_streaming.rs \
        backend/actio-core/src/engine/inference_pipeline.rs \
        backend/actio-core/src/engine/mod.rs
git commit -m "feat(engine): live_streaming service, no-DB-write streaming path"
```

---

## Task 12: pipeline_supervisor refactor — three subsystems, retire IDLE_GRACE

**Files:**
- Modify: `backend/actio-core/src/lib.rs`

- [ ] **Step 1: Define the supervisor's new API**

In `lib.rs`, replace `pipeline_supervisor` with:

```rust
const SUPERVISOR_TICK: std::time::Duration = std::time::Duration::from_secs(5);

async fn pipeline_supervisor(state: AppState) {
    info!("Supervising capture daemon, batch processor, live streaming");
    let mut prev_always_listening: Option<bool> = None;

    loop {
        tokio::time::sleep(SUPERVISOR_TICK).await;
        let settings = state.settings_manager.get().await;
        let always_listening = settings.audio.always_listening;

        // 1) CaptureDaemon — running iff (always_listening && !muted) || live_active.
        let live_active = state.live_streaming.is_running().await;
        let muted = state.capture_daemon.is_muted().await;
        let want_capture = (always_listening && !muted) || live_active;
        let have_capture = !state.capture_daemon.is_muted().await
            && /* daemon was started at least once */ prev_always_listening.is_some();

        if want_capture && !have_capture {
            if let Err(e) = state.capture_daemon.start().await {
                warn!(error=%e, "CaptureDaemon failed to start");
            }
        } else if !want_capture && prev_always_listening == Some(true) && !live_active {
            state.capture_daemon.stop().await;
        }
        prev_always_listening = Some(always_listening);

        // 2) BatchProcessor — runs while always_listening is true.
        if always_listening {
            state.batch_processor.ensure_running().await;
        } else {
            state.batch_processor.ensure_stopped().await;
        }

        // 3) LiveStreamingService is started/stopped by the dictation/
        //    translation HTTP handlers directly (see api/session.rs and
        //    api/translation.rs). The supervisor only observes its state.
    }
}
```

- [ ] **Step 2: Drop `IDLE_GRACE_PERIOD` and the old supervisor fields**

Delete the `IDLE_GRACE_PERIOD` constant and any subscriber-count-tracking fields from `AppState`. The new supervisor doesn't read subscriber counts.

- [ ] **Step 3: Wire the new fields into AppState**

In whichever module declares `AppState` (likely `lib.rs` or `state.rs`), add:

```rust
pub capture_daemon: Arc<crate::engine::capture_daemon::CaptureDaemon>,
pub batch_processor: Arc<crate::engine::batch_processor::BatchProcessorHandle>,
pub live_streaming: Arc<crate::engine::live_streaming::LiveStreamingService>,
```

Where `BatchProcessorHandle` is a thin wrapper added in this step. Append to `engine/batch_processor.rs`:

```rust
pub struct BatchProcessorHandle {
    inner: tokio::sync::Mutex<HandleInner>,
    state_for_loop: parking_lot::Mutex<Option<crate::AppState>>,
}

struct HandleInner {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}

impl BatchProcessorHandle {
    pub fn new() -> Self {
        Self {
            inner: tokio::sync::Mutex::new(HandleInner { cancel: None }),
            state_for_loop: parking_lot::Mutex::new(None),
        }
    }
    pub fn set_state(&self, state: crate::AppState) {
        *self.state_for_loop.lock() = Some(state);
    }
    pub async fn ensure_running(&self) {
        let mut g = self.inner.lock().await;
        if g.cancel.is_some() { return; }
        let state = match self.state_for_loop.lock().clone() {
            Some(s) => s,
            None => return,
        };
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        g.cancel = Some(cancel_tx);
        tokio::spawn(async move {
            // Periodic claim → process loop. Cold-load ASR per clip; unload after.
            loop {
                if cancel_rx.try_recv().is_ok() { break; }
                match audio_clip::claim_next_pending(&state.pool).await {
                    Ok(Some(clip)) => {
                        if let Err(e) = process_one_with_state(&state, &clip).await {
                            warn!(error=%e, clip_id=%clip.id, "batch processor: clip failed");
                            let _ = audio_clip::mark_failed(&state.pool, clip.id, &e.to_string()).await;
                        }
                    }
                    Ok(None) => tokio::time::sleep(std::time::Duration::from_secs(5)).await,
                    Err(e) => {
                        warn!(error=%e, "batch processor: claim failed");
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                    }
                }
            }
            info!("BatchProcessor stopped");
        });
    }
    pub async fn ensure_stopped(&self) {
        let mut g = self.inner.lock().await;
        if let Some(tx) = g.cancel.take() { let _ = tx.send(()); }
    }
}

async fn process_one_with_state(
    state: &crate::AppState,
    clip: &AudioClip,
) -> anyhow::Result<()> {
    let settings = state.settings_manager.get().await;
    let archive_model = settings.audio.resolved_asr_models().archive
        .ok_or_else(|| anyhow::anyhow!("no archive ASR model configured"))?;
    let asr = SherpaArchiveAsr {
        model_id: archive_model.clone(),
        model_paths: state.model_paths.clone(),
    };
    let embedder = crate::engine::diarization::SherpaSegmentEmbedder::new(state)?;
    let cfg = ClusteringConfig {
        cosine_threshold: settings.audio.cluster_cosine_threshold,
        min_segments_per_cluster: 1,
    };
    process_clip_with_clustering(&state.pool, &asr, &embedder, clip, &cfg).await?;
    Ok(())
}
```

> `SherpaSegmentEmbedder::new(state)` is a thin wrapper around the existing `EMBEDDING_WORKERS` LRU in `diarization.rs`. If `diarization.rs` doesn't yet expose a per-segment-batch entry point, add a `pub fn embed_batch(model_id, paths, segments) -> Result<Vec<(Uuid, Vec<f32>)>>` there mirroring the existing single-segment call.

- [ ] **Step 4: Wire `set_state` at boot**

In `start_server` (or wherever `AppState` is constructed and `pipeline_supervisor` is spawned), call `state.batch_processor.set_state(state.clone())` before spawning the supervisor. This breaks the "the loop needs an AppState that contains the loop's own handle" cycle.

- [ ] **Step 5: Compile and run tests**

```bash
cargo check -p actio-core --tests
cargo test -p actio-core --lib
```

Expected: all pre-existing tests + new ones pass.

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/lib.rs backend/actio-core/src/engine/batch_processor.rs
git commit -m "refactor(supervisor): three-subsystem supervisor; retire IDLE_GRACE_PERIOD"
```

---

## Task 13: Privacy mode — wiring `always_listening = false`

**Files:**
- Modify: `backend/actio-core/src/api/session.rs`
- Modify: `backend/actio-core/src/api/translation.rs` (or wherever live translation is wired)

The session/translation handlers must start the CaptureDaemon explicitly when starting a live mode if `always_listening = false`. Otherwise no audio flows during dictation/translation. They must also tell the daemon "don't write segments to disk" while privacy mode is on.

- [ ] **Step 1: Add an `archive_enabled` flag to CaptureDaemon**

In `engine/capture_daemon.rs`:

```rust
impl CaptureDaemon {
    pub async fn set_archive_enabled(&self, enabled: bool) {
        let mut g = self.inner.lock().await;
        g.archive_enabled = enabled;
    }
    pub async fn archive_enabled(&self) -> bool {
        self.inner.lock().await.archive_enabled
    }
}
// In Inner: add `archive_enabled: bool` (default true).
```

In `engine/clip_writer.rs::run_clip_writer_loop`, at the top of the loop body, drop the event if `!capture.archive_enabled()`:

```rust
if !capture_clone.archive_enabled().await { continue; }
```

(Pass the daemon handle into `run_clip_writer_loop` for this check.)

- [ ] **Step 2: Update the supervisor**

In the new `pipeline_supervisor`, set `archive_enabled` from `always_listening`:

```rust
state.capture_daemon.set_archive_enabled(always_listening).await;
```

This lets the daemon stay running while a live mode is active in privacy mode without producing clips.

- [ ] **Step 3: Update dictation/translation start handlers**

Wherever dictation start runs today (`api/session.rs` `start_dictation` or similar), make it call `state.capture_daemon.start().await?` *before* `state.live_streaming.start(...)`, regardless of `always_listening`. Mirror in the translation endpoint.

On stop: `state.live_streaming.stop().await`. Don't stop the capture daemon if `always_listening = true` (the supervisor manages it). If `always_listening = false`, supervisor's next tick will stop it.

- [ ] **Step 4: Add an integration test for privacy mode**

In `lib.rs` test module or a new `tests/privacy_mode.rs`:

```rust
#[tokio::test]
async fn privacy_mode_does_not_create_audio_clips() {
    let pool = test_pool().await;
    // Set always_listening = false
    // Start dictation
    // Feed a fake speech segment to capture_daemon (via a test-only helper)
    // Stop dictation
    // Assert: zero rows in audio_clips
    // (Implementation depends on how the test harness can stub cpal.)
}
```

> If full cpal stubbing is too invasive, accept this as a manual smoke step in Task 18 instead and remove this Step 4.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/capture_daemon.rs \
        backend/actio-core/src/engine/clip_writer.rs \
        backend/actio-core/src/lib.rs \
        backend/actio-core/src/api/session.rs \
        backend/actio-core/src/api/translation.rs
git commit -m "feat(privacy): always_listening=false gates archive writes only"
```

---

## Task 14: Audio retention sweep + provisional speaker GC

**Files:**
- Modify: `backend/actio-core/src/engine/clip_storage.rs`
- Modify: `backend/actio-core/src/repository/speaker.rs`

- [ ] **Step 1: Generalize `clip_storage.rs::start_cleanup_task`**

Current implementation sweeps a flat dir for files older than N days. New implementation must walk one extra level deep (`<clips_dir>/<session>/<clip>/*.wav`) and delete the whole `<clip>` dir when its mtime is past the cutoff.

```rust
pub fn start_cleanup_task(dir: PathBuf, retention_days: u32) {
    tokio::spawn(async move {
        sweep_clip_dirs(&dir, retention_days);
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            sweep_clip_dirs(&dir, retention_days);
        }
    });
}

fn sweep_clip_dirs(dir: &Path, retention_days: u32) {
    let cutoff = match SystemTime::now()
        .checked_sub(Duration::from_secs(retention_days as u64 * 86_400)) {
        Some(t) => t,
        None => { warn!("retention cutoff underflow"); return; }
    };
    let sessions = match std::fs::read_dir(dir) { Ok(r) => r, Err(_) => return };
    for s in sessions.flatten() {
        let session_path = s.path();
        if !session_path.is_dir() { continue; }
        let clips = match std::fs::read_dir(&session_path) { Ok(r) => r, Err(_) => continue };
        for c in clips.flatten() {
            let cp = c.path();
            if !cp.is_dir() { continue; }
            let modified = c.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::now());
            if modified < cutoff {
                if let Err(e) = std::fs::remove_dir_all(&cp) {
                    warn!(error=%e, path=?cp, "failed to remove stale clip dir");
                } else {
                    debug!(path=?cp, "removed stale clip dir");
                }
            }
        }
    }
}
```

Drop the old per-WAV sweep — anything left in the legacy flat-dir layout becomes orphan and will be picked up by the new sweep on subsequent cycles.

- [ ] **Step 2: Add provisional speaker GC**

Append to `repository/speaker.rs`:

```rust
pub async fn gc_stale_provisionals(
    pool: &sqlx::SqlitePool,
    older_than_days: i64,
) -> Result<u64, sqlx::Error> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days);
    let cutoff_str = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let res = sqlx::query(
        r#"DELETE FROM speakers
           WHERE kind = 'provisional'
             AND (provisional_last_matched_at IS NULL
                  OR provisional_last_matched_at < ?1)"#,
    )
    .bind(cutoff_str).execute(pool).await?;
    Ok(res.rows_affected())
}
```

- [ ] **Step 3: Schedule it from boot**

In `lib.rs::start_server`, alongside the audio cleanup task:

```rust
let pool = state.pool.clone();
let days = state.settings_manager.get().await.audio.provisional_voiceprint_gc_days as i64;
tokio::spawn(async move {
    loop {
        match crate::repository::speaker::gc_stale_provisionals(&pool, days).await {
            Ok(0) => {}
            Ok(n) => info!(count=n, "GC'd stale provisional speakers"),
            Err(e) => warn!(error=%e, "provisional GC failed"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(86_400)).await;
    }
});
```

- [ ] **Step 4: Test the GC**

Append to `repository/speaker.rs` test module:

```rust
#[tokio::test]
async fn gc_removes_only_old_unmatched_provisionals() {
    let pool = test_pool().await;
    let tenant = Uuid::nil();
    let id_old = Uuid::new_v4();
    insert_provisional(&pool, id_old, tenant, "old").await.unwrap();
    sqlx::query(
        r#"UPDATE speakers SET provisional_last_matched_at = '2020-01-01T00:00:00.000Z' WHERE id = ?1"#,
    ).bind(id_old.to_string()).execute(&pool).await.unwrap();
    let id_new = Uuid::new_v4();
    insert_provisional(&pool, id_new, tenant, "new").await.unwrap();

    let n = gc_stale_provisionals(&pool, 30).await.unwrap();
    assert_eq!(n, 1);
    let remaining = list_provisional(&pool).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, id_new);
}
```

Run: `cargo test -p actio-core --lib gc_removes_only_old_unmatched_provisionals`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add backend/actio-core/src/engine/clip_storage.rs \
        backend/actio-core/src/repository/speaker.rs \
        backend/actio-core/src/lib.rs
git commit -m "feat(retention): nested clip-dir sweep + provisional speaker GC"
```

---

## Task 15: Candidate Speakers panel — list / promote / dismiss endpoints

**Files:**
- Create: `backend/actio-core/src/api/candidate_speaker.rs`
- Modify: `backend/actio-core/src/api/mod.rs`
- Modify: `backend/actio-core/src/repository/speaker.rs`

- [ ] **Step 1: Add the repo helpers**

```rust
pub async fn promote_provisional(
    pool: &sqlx::SqlitePool,
    id: Uuid,
    new_display_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE speakers
           SET kind = 'enrolled',
               display_name = ?2,
               provisional_last_matched_at = NULL
           WHERE id = ?1 AND kind = 'provisional'"#,
    )
    .bind(id.to_string()).bind(new_display_name).execute(pool).await?;
    Ok(())
}

pub async fn dismiss_provisional(pool: &sqlx::SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    // Hard-delete; segments' speaker_id becomes NULL via existing
    // ON DELETE SET NULL FK behaviour.
    sqlx::query(r#"DELETE FROM speakers WHERE id = ?1 AND kind = 'provisional'"#)
        .bind(id.to_string()).execute(pool).await?;
    Ok(())
}
```

- [ ] **Step 2: Add the routes**

```rust
//! HTTP routes for the Candidate Speakers panel — provisional speaker
//! management. Lists provisional rows, promotes one to enrolled with a
//! user-supplied name, or dismisses (deletes).

use axum::{extract::{Path, State}, Json, Router};
use axum::routing::{get, post, delete};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::repository::speaker;
use crate::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct CandidateSpeaker {
    pub id: Uuid,
    pub display_name: String,
    pub last_matched_at: Option<String>,
}

#[utoipa::path(get, path = "/candidate-speakers",
    responses((status = 200, body = Vec<CandidateSpeaker>)))]
pub async fn list_candidates(
    State(state): State<AppState>,
) -> Result<Json<Vec<CandidateSpeaker>>, axum::http::StatusCode> {
    let rows = speaker::list_provisional(&state.pool)
        .await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(rows.into_iter().map(|r| CandidateSpeaker {
        id: r.id, display_name: r.display_name, last_matched_at: r.last_matched_at,
    }).collect()))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PromoteBody { pub display_name: String }

#[utoipa::path(post, path = "/candidate-speakers/{id}/promote",
    request_body = PromoteBody,
    responses((status = 204, description = "Promoted")))]
pub async fn promote(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PromoteBody>,
) -> Result<axum::http::StatusCode, axum::http::StatusCode> {
    speaker::promote_provisional(&state.pool, id, &body.display_name)
        .await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/candidate-speakers/{id}",
    responses((status = 204, description = "Dismissed")))]
pub async fn dismiss(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, axum::http::StatusCode> {
    speaker::dismiss_provisional(&state.pool, id)
        .await.map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/candidate-speakers", get(list_candidates))
        .route("/candidate-speakers/:id/promote", post(promote))
        .route("/candidate-speakers/:id", delete(dismiss))
}
```

In `api/mod.rs`, add `pub mod candidate_speaker;` and merge `candidate_speaker::router()` into the main app.

Add the three routes to the utoipa OpenAPI listing (mirror the existing pattern in api/mod.rs).

- [ ] **Step 3: Test promote**

Add to `speaker.rs` test module:

```rust
#[tokio::test]
async fn promote_provisional_renames_and_clears_last_matched_at() {
    let pool = test_pool().await;
    let tenant = Uuid::nil();
    let id = Uuid::new_v4();
    insert_provisional(&pool, id, tenant, "Unknown 2026-04-25").await.unwrap();
    promote_provisional(&pool, id, "Bob").await.unwrap();
    let found = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"SELECT display_name, kind, provisional_last_matched_at FROM speakers WHERE id = ?1"#,
    ).bind(id.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(found.0, "Bob");
    assert_eq!(found.1, "enrolled");
    assert!(found.2.is_none());
}
```

Run: `cargo test -p actio-core --lib promote_provisional_renames_and_clears_last_matched_at`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/api/candidate_speaker.rs \
        backend/actio-core/src/api/mod.rs \
        backend/actio-core/src/repository/speaker.rs
git commit -m "feat(api): candidate speakers panel — list/promote/dismiss"
```

---

## Task 16: Trace endpoint fallback — audio_clips first, extraction_windows second

**Files:**
- Modify: `backend/actio-core/src/api/reminder.rs`

- [ ] **Step 1: Locate the existing `/reminders/{id}/trace` handler**

It currently builds a "show context" payload by reading the reminder's `source_window_id` from `extraction_windows`. Adapt it.

- [ ] **Step 2: Update the lookup**

```rust
// New: look up source first in audio_clips (post-migration rows),
// fall back to extraction_windows (legacy rows).
let trace = if let Some(window_id) = reminder.source_window_id {
    if let Some(clip) = crate::repository::audio_clip::get_by_id(&state.pool, window_id).await? {
        TraceResponse::from_clip(&state.pool, clip).await?
    } else if let Some(legacy) = crate::repository::extraction_window::get_by_id(&state.pool, window_id).await? {
        TraceResponse::from_extraction_window(&state.pool, legacy).await?
    } else {
        return Err(axum::http::StatusCode::NOT_FOUND);
    }
} else {
    return Err(axum::http::StatusCode::NOT_FOUND);
};
```

`TraceResponse::from_clip` builds the same `[HH:MM:SS • Speaker]: text` shape as `from_extraction_window`, but reads transcripts via `transcript::list_for_clip` instead of by `(session_id, start_ms..end_ms)`.

- [ ] **Step 3: Test**

Add a test that creates a reminder with `source_window_id = clip_id` and verifies the trace endpoint returns the clip's transcripts. (Existing test fixtures probably already cover the legacy path.)

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/api/reminder.rs
git commit -m "feat(trace): fallback lookup audio_clips → extraction_windows"
```

---

## Task 17: Retire deprecated settings + legacy code paths

**Files:**
- Modify: `backend/actio-core/src/engine/app_settings.rs`
- Modify: `backend/actio-core/src/engine/window_extractor.rs`
- Modify: `backend/actio-core/src/engine/clip_storage.rs`
- Modify: `backend/actio-core/src/lib.rs`
- Modify: `frontend/src/api/actio-api.ts` (and any settings UI)

- [ ] **Step 1: Drop deprecated AudioSettings fields**

Remove `window_length_ms`, `window_step_ms`, `extraction_tick_secs`, `clip_retention_days`, and `asr_model`. Update `AudioSettingsPatch` and Default impl. The corresponding default fns also delete.

> Settings persisted on disk before this change have these fields; serde will ignore unknown fields by default, so old settings files still load. The `resolved_asr_models` shim from Task 2 was the read-time migration; if the user's settings still contain only `asr_model`, the Settings UI must surface a warning to pick `live_asr_model` and `archive_asr_model` explicitly. (Out of scope for this plan — log a `tracing::warn!` instead.)

- [ ] **Step 2: Drop legacy code paths**

- Delete the streaming "save voiceprint candidate clip on speaker-id failure" code path inside the old InferencePipeline (already removed when `inference_pipeline.rs` shrank in Task 11; verify no orphan references).
- Delete `clip_storage::write_clip` if no caller remains.
- Delete `window_extractor::run_extraction_loop` and remove its spawn from `lib.rs::start_server`.

- [ ] **Step 3: Compile + test**

```bash
cargo check -p actio-core --tests
cargo test -p actio-core --lib
```

Expected: clean.

- [ ] **Step 4: Sync the frontend type for AudioSettings**

In `frontend/src/api/actio-api.ts` (or wherever `AudioSettings` is mirrored), drop the same five fields and add the new ones. If a Settings UI component referenced any of the dropped fields, replace with the corresponding new field (e.g. `clip_target_secs` instead of `window_length_ms`).

```bash
cd frontend && pnpm build && pnpm test
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: retire window_extractor scheduler + deprecated settings"
```

---

## Task 18: End-to-end smoke test (manual + automated where feasible)

**Files:**
- Create: `backend/actio-core/tests/batch_clip_processing_smoke.rs`

- [ ] **Step 1: Write an integration test that drives the full pipeline with stubs**

```rust
//! End-to-end batch pipeline test using stub ASR + stub embedder. Verifies
//! that a fake "session with two clips" produces the expected DB state.

use actio_core::engine::batch_processor::{
    process_clip_with_clustering, ArchiveAsr, ArchiveTranscript, ClusteringConfig, SegmentEmbedder,
};
use actio_core::engine::clip_writer::write_manifest;
use actio_core::domain::types::{ClipManifest, ClipManifestSegment};
use actio_core::repository::{audio_clip, segment, session, speaker};
use tempfile::tempdir;
use uuid::Uuid;

// Reuse the in-memory test pool helper from the lib crate.
use actio_core::repository::db::test_helpers::test_pool;

struct StubAsr;
impl ArchiveAsr for StubAsr {
    fn transcribe_clip(&self, m: &ClipManifest, _: &std::path::Path)
      -> anyhow::Result<Vec<ArchiveTranscript>>
    {
        Ok(m.segments.iter().map(|s| ArchiveTranscript {
            segment_id: s.id, start_ms: s.start_ms, end_ms: s.end_ms,
            text: format!("hello {}", s.start_ms),
        }).collect())
    }
}

struct StubEmbedder { vecs: Vec<Vec<f32>> }
impl SegmentEmbedder for StubEmbedder {
    fn embed_segments(&self, m: &ClipManifest, _: &std::path::Path)
      -> anyhow::Result<Vec<(Uuid, Vec<f32>)>> {
        Ok(m.segments.iter().enumerate()
            .map(|(i, s)| (s.id, self.vecs[i].clone())).collect())
    }
    fn dimension(&self) -> i64 { 2 }
}

#[tokio::test]
async fn two_clips_with_same_speaker_link_via_provisional() {
    let pool = test_pool().await;
    let session_id = session::create_default_session(&pool).await.unwrap();
    let cfg = ClusteringConfig { cosine_threshold: 0.4, min_segments_per_cluster: 1 };

    let tmp1 = tempdir().unwrap();
    let m1 = ClipManifest { clip_id: Uuid::new_v4(), session_id, started_at_ms: 0, ended_at_ms: 300_000,
        segments: vec![ClipManifestSegment { id: Uuid::new_v4(), start_ms: 0, end_ms: 1_000, file: "a.wav".into() }]};
    let p1 = write_manifest(tmp1.path(), &m1).unwrap();
    audio_clip::insert_pending(&pool, session_id, 0, 300_000, 1, p1.to_string_lossy().as_ref()).await.unwrap();
    let c1 = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
    process_clip_with_clustering(&pool, &StubAsr, &StubEmbedder { vecs: vec![vec![1.0, 0.0]] }, &c1, &cfg).await.unwrap();

    let tmp2 = tempdir().unwrap();
    let m2 = ClipManifest { clip_id: Uuid::new_v4(), session_id, started_at_ms: 300_000, ended_at_ms: 600_000,
        segments: vec![ClipManifestSegment { id: Uuid::new_v4(), start_ms: 300_000, end_ms: 301_000, file: "b.wav".into() }]};
    let p2 = write_manifest(tmp2.path(), &m2).unwrap();
    audio_clip::insert_pending(&pool, session_id, 300_000, 600_000, 1, p2.to_string_lossy().as_ref()).await.unwrap();
    let c2 = audio_clip::claim_next_pending(&pool).await.unwrap().unwrap();
    process_clip_with_clustering(&pool, &StubAsr, &StubEmbedder { vecs: vec![vec![0.99, 0.14]] }, &c2, &cfg).await.unwrap();

    let provisional = speaker::list_provisional(&pool).await.unwrap();
    assert_eq!(provisional.len(), 1, "second clip should reuse the first's provisional row");

    let segs1 = segment::list_for_clip(&pool, c1.id).await.unwrap();
    let segs2 = segment::list_for_clip(&pool, c2.id).await.unwrap();
    assert_eq!(segs1[0].speaker_id, segs2[0].speaker_id);
}
```

Run: `cargo test -p actio-core --test batch_clip_processing_smoke`
Expected: PASS.

- [ ] **Step 2: Manual smoke checklist**

Document this in the PR description and run it locally before merge:

1. `cargo run --bin actio-asr` — server boots clean.
2. Open the frontend; mute is off, `always_listening = true`.
3. Speak through a 7-min session with two distinct speakers.
4. After ~5 min, verify on disk: `<clips_dir>/<session>/<clip1>/seg_*.wav` and `manifest.json` exist.
5. After clip 1 processes, verify `audio_clips.status = 'processed'` and a new `speakers` row with `kind = 'provisional'`.
6. After clip 2 processes, verify the second clip reuses the same `speaker_id` for the same person — count of `kind='provisional'` rows hasn't doubled.
7. Reminders for clip 1 are visible on the Board with `source_window_id` resolving via the trace endpoint to the clip's transcripts.
8. Toggle mute mid-clip — verify the in-flight clip closes early and a fresh clip starts on unmute.
9. Set `always_listening = false`, restart, start dictation — verify *no* new `audio_clips` rows are written but live transcripts still broadcast on `/ws`.
10. Memory at idle (no dictation/translation, between clips): RSS should not include the archive ASR model.

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/tests/batch_clip_processing_smoke.rs
git commit -m "test(smoke): two-clip cross-clip provisional linking"
```

---

## Self-Review Notes

Items intentionally deferred (carried as inline TODO comments in code, not as plan placeholders):

- **`SherpaArchiveAsr::cold_recognizer` and `decode_full`** in `engine/asr.rs` — the existing ASR module is streaming-first; the batch wrapper feeds a buffer in one shot. Implementer must check `engine/asr.rs` for the exact API and add a thin batch helper there. (Task 8.)
- **`SherpaSegmentEmbedder::embed_batch`** in `engine/diarization.rs` — current code embeds one segment at a time; the batch caller wants `Vec<(Uuid, Vec<f32>)>`. Add a thin loop on top of the existing single-segment call (the EMBEDDING_WORKERS LRU already serializes correctly). (Task 12.)
- **`SileroSession::feed`** wrapper in `engine/vad.rs` — the precise existing API may differ; adapt the bridge thread in capture_daemon.rs accordingly. (Task 6.)
- **`OutgoingFrame` in `api/ws.rs`** — referenced from `live_streaming.rs`; wire to whatever WS broadcast type the live transcript path uses today.
- **`run_streaming_loop` body** in `live_streaming.rs` — port the per-segment loop from the old `inference_pipeline.rs`, removing every DB write. (Task 11.)

These are expected adapter gaps — the implementer will resolve them by reading the matching existing module and writing a small adapter, not by inventing new infrastructure. The skill name for them is "implementation glue", not "design TBD".

---

## Execution Notes

The plan is written for a fresh worktree at `D:/Dev/Actio/.worktrees/batch-clip-processing` on branch `feat/batch-clip-processing`. All `git commit` steps should run from inside the worktree. Backend commands (`cargo …`) run from `<worktree>/backend`; frontend commands (`pnpm …`) run from `<worktree>/frontend`.

Tasks 1–9 form the foundation and can be executed straight through with high confidence.
Tasks 10–13 are the integration phase — be ready to refactor existing call sites and run the full test suite frequently.
Tasks 14–18 are polish + cleanup; safe to land in any order once 13 is green.
