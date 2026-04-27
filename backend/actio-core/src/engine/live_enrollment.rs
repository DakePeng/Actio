//! Live voiceprint enrollment — lets the user read prepared passages and
//! have the already-running backend audio pipeline capture them, rather
//! than opening a second audio stream in the browser.
//!
//! While a session is active, `handle_segment_embedding` in
//! `inference_pipeline.rs` checks the shared `LiveEnrollment` state. If it
//! is `Active` and the incoming VAD segment passes quality + duration
//! gates, the segment's embedding is saved against the target speaker and
//! the `captured` counter increments. Once `captured == target`, the state
//! transitions to `Complete` and the next segment resumes normal
//! identification.

use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Shared state handle — cheap to clone and pass through AppState.
pub type LiveEnrollment = Arc<Mutex<Option<EnrollmentState>>>;

pub fn new_state() -> LiveEnrollment {
    Arc::new(Mutex::new(None))
}

#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct EnrollmentState {
    pub speaker_id: String,
    pub target: u32,
    pub captured: u32,
    /// Last duration captured — the frontend uses this to briefly flash
    /// "captured clip N" feedback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_captured_duration_ms: Option<f64>,
    pub status: Status,
    /// Monotonic version that bumps on every meaningful mutation (capture,
    /// reject, status change). NOT bumped on raw level updates so a quiet
    /// session doesn't spin the counter at audio-chunk rate.
    pub version: u64,
    /// Smoothed RMS of the latest audio chunks. Drives the live mic-level
    /// meter in the enrollment UI.
    #[serde(default)]
    pub rms_level: f32,
    /// Most recent reason a VAD segment was rejected by the quality gates —
    /// `too_short`, `too_long`, or `low_quality`. Cleared when a subsequent
    /// segment is accepted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_rejected_reason: Option<String>,
    /// IDs of `speaker_embeddings` rows saved during this session. If the
    /// user cancels mid-enrollment we delete exactly these rows instead of
    /// every row for the speaker — otherwise a cancel would wipe out prior
    /// successful enrollments too. Not serialized to the frontend.
    #[serde(skip)]
    pub saved_embedding_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Complete,
    Cancelled,
}

pub async fn start(
    slot: &LiveEnrollment,
    speaker_id: Uuid,
    target: u32,
) -> Result<EnrollmentState, &'static str> {
    let mut guard = slot.lock().await;
    if let Some(state) = guard.as_ref() {
        if state.status == Status::Active {
            return Err("enrollment_already_active");
        }
    }
    let new_state = EnrollmentState {
        speaker_id: speaker_id.to_string(),
        target,
        captured: 0,
        last_captured_duration_ms: None,
        status: Status::Active,
        version: 1,
        rms_level: 0.0,
        last_rejected_reason: None,
        saved_embedding_ids: Vec::new(),
    };
    *guard = Some(new_state.clone());
    Ok(new_state)
}

pub async fn cancel(slot: &LiveEnrollment) -> Option<EnrollmentState> {
    let mut guard = slot.lock().await;
    if let Some(state) = guard.as_mut() {
        if state.status == Status::Active {
            state.status = Status::Cancelled;
            state.version += 1;
            return Some(state.clone());
        }
    }
    None
}

/// Delete any embedding rows that were saved during an incomplete
/// enrollment. Called from the cancel handler so a half-finished capture
/// doesn't leave biased partial voiceprints in the database. Returns the
/// number of rows removed. Prior successful enrollments for the same
/// speaker are preserved because we only delete the specific IDs we
/// recorded during this session.
pub async fn cleanup_partial_embeddings(
    slot: &LiveEnrollment,
    pool: &sqlx::SqlitePool,
) -> anyhow::Result<usize> {
    let ids: Vec<String> = {
        let mut guard = slot.lock().await;
        match guard.as_mut() {
            Some(state) if state.status != Status::Complete => {
                std::mem::take(&mut state.saved_embedding_ids)
            }
            _ => return Ok(0),
        }
    };
    if ids.is_empty() {
        return Ok(0);
    }
    let mut removed = 0usize;
    for id in &ids {
        let result = sqlx::query("DELETE FROM speaker_embeddings WHERE id = ?1")
            .bind(id)
            .execute(pool)
            .await?;
        removed += result.rows_affected() as usize;
    }
    tracing::info!(
        removed,
        attempted = ids.len(),
        "Cleaned up partial enrollment embeddings"
    );
    Ok(removed)
}

/// Returns true if enrollment finished naturally (`Status::Complete`) and
/// the caller should tear down any pipeline resources they spun up to drive
/// it. Safe to call repeatedly — the completion is a terminal state and the
/// frontend polls status anyway, so making teardown idempotent at this layer
/// lets `get_live_enrollment_status` drive it without extra plumbing.
pub async fn is_complete(slot: &LiveEnrollment) -> bool {
    matches!(
        slot.lock().await.as_ref().map(|s| &s.status),
        Some(Status::Complete)
    )
}

pub async fn snapshot(slot: &LiveEnrollment) -> Option<EnrollmentState> {
    slot.lock().await.clone()
}

/// Publish a smoothed RMS level sampled from the audio capture tap.
/// Cheap hot-path — try-lock only, no version bump, no-op when no
/// enrollment is armed.
pub fn publish_level(slot: &LiveEnrollment, rms: f32) {
    let Ok(mut guard) = slot.try_lock() else {
        return;
    };
    if let Some(state) = guard.as_mut() {
        if state.status == Status::Active {
            state.rms_level = rms;
        }
    }
}

/// Called from the pipeline after a segment's embedding has been extracted.
/// Returns `Some(speaker_id)` if the segment was consumed by enrollment
/// (in which case the caller should skip the normal identify + candidate
/// retention path). Returns `None` if enrollment is inactive or the clip
/// didn't meet the bar — normal processing should continue.
pub async fn consume_segment(
    slot: &LiveEnrollment,
    duration_ms: f64,
    quality_score: f32,
    pool: &sqlx::SqlitePool,
    embedding: &[f32],
) -> anyhow::Result<Option<String>> {
    const MIN_DURATION_MS: f64 = 3_000.0;
    const MAX_DURATION_MS: f64 = 30_000.0;
    const MIN_QUALITY: f32 = 0.6;

    // Every decision — active check, gate evaluation, counter bump — happens
    // inside a single lock critical section so a concurrent cancel can't
    // race between a snapshot-taken-outside-the-lock and the mutation that
    // follows. The previous shape did the gate check against a stale
    // snapshot and relied on `mark_rejected` re-checking under the lock.
    let claim = {
        let mut guard = slot.lock().await;
        let Some(state) = guard.as_mut() else {
            return Ok(None);
        };
        if state.status != Status::Active {
            return Ok(None);
        }
        if duration_ms < MIN_DURATION_MS {
            state.last_rejected_reason = Some("too_short".to_string());
            state.version += 1;
            return Ok(None);
        }
        if duration_ms > MAX_DURATION_MS {
            state.last_rejected_reason = Some("too_long".to_string());
            state.version += 1;
            return Ok(None);
        }
        if quality_score < MIN_QUALITY {
            state.last_rejected_reason = Some("low_quality".to_string());
            state.version += 1;
            return Ok(None);
        }
        let Ok(uuid) = Uuid::parse_str(&state.speaker_id) else {
            return Ok(None);
        };
        let is_first = state.captured == 0;
        state.captured += 1;
        state.last_captured_duration_ms = Some(duration_ms);
        state.last_rejected_reason = None;
        state.version += 1;
        let reached = state.captured >= state.target;
        if reached {
            state.status = Status::Complete;
            state.version += 1;
        }
        (uuid, is_first, reached)
    };

    let (speaker_uuid, is_first, target_reached) = claim;

    // Persist as a speaker_embeddings row. First capture becomes primary.
    let embedding_id = crate::domain::speaker_matcher::save_embedding(
        pool,
        speaker_uuid,
        embedding,
        duration_ms,
        quality_score as f64,
        is_first,
    )
    .await?;

    // Record the saved ID so `cleanup_partial_embeddings` can scope deletion
    // to this session on cancel. Only retain the list while the session is
    // still in-flight — once it Completes we don't want cancel-after-complete
    // (which would be a no-op anyway) to wipe the legitimate voiceprint.
    {
        let mut guard = slot.lock().await;
        if let Some(state) = guard.as_mut() {
            if state.status == Status::Active {
                state.saved_embedding_ids.push(embedding_id.to_string());
            } else if state.status == Status::Complete {
                // Session just completed; clear the staging list so a stray
                // cancel can't clobber these rows.
                state.saved_embedding_ids.clear();
            }
        }
    }

    let _ = target_reached; // already reflected in state
    Ok(Some(speaker_uuid.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;

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

    /// Insert a speaker so FK constraints on speaker_embeddings hold.
    async fn mk_speaker(pool: &SqlitePool) -> Uuid {
        let s = crate::repository::speaker::create_speaker(
            pool,
            "Test Speaker",
            "#E57373",
            Uuid::nil(),
        )
        .await
        .unwrap();
        Uuid::parse_str(&s.id).unwrap()
    }

    fn good_embedding() -> Vec<f32> {
        // 192-dim is the most common embedding size in the catalog.
        let mut v = vec![0.0_f32; 192];
        v[0] = 1.0;
        v
    }

    #[tokio::test]
    async fn consume_segment_with_no_session_returns_none() {
        let pool = fresh_pool().await;
        let slot = new_state();
        let out = consume_segment(&slot, 5_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        assert!(out.is_none());
        // Slot stays empty.
        assert!(snapshot(&slot).await.is_none());
    }

    #[tokio::test]
    async fn consume_segment_bails_when_status_not_active() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        start(&slot, speaker_id, 3).await.unwrap();
        cancel(&slot).await; // status → Cancelled

        let out = consume_segment(&slot, 5_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        assert!(out.is_none());
        let snap = snapshot(&slot).await.unwrap();
        assert_eq!(snap.captured, 0);
    }

    #[tokio::test]
    async fn consume_segment_rejects_too_short_with_reason_and_version_bump() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        let initial = start(&slot, speaker_id, 3).await.unwrap();

        let out = consume_segment(&slot, 1_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        assert!(out.is_none());

        let snap = snapshot(&slot).await.unwrap();
        assert_eq!(snap.captured, 0);
        assert_eq!(snap.last_rejected_reason.as_deref(), Some("too_short"));
        assert!(snap.version > initial.version);
    }

    #[tokio::test]
    async fn consume_segment_rejects_too_long() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        start(&slot, speaker_id, 3).await.unwrap();

        let out = consume_segment(&slot, 60_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        assert!(out.is_none());
        let snap = snapshot(&slot).await.unwrap();
        assert_eq!(snap.last_rejected_reason.as_deref(), Some("too_long"));
        assert_eq!(snap.captured, 0);
    }

    #[tokio::test]
    async fn consume_segment_rejects_low_quality() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        start(&slot, speaker_id, 3).await.unwrap();

        let out = consume_segment(&slot, 5_000.0, 0.3, &pool, &good_embedding())
            .await
            .unwrap();
        assert!(out.is_none());
        let snap = snapshot(&slot).await.unwrap();
        assert_eq!(snap.last_rejected_reason.as_deref(), Some("low_quality"));
        assert_eq!(snap.captured, 0);
    }

    #[tokio::test]
    async fn consume_segment_accept_bumps_counter_clears_reason_records_id() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        start(&slot, speaker_id, 3).await.unwrap();

        // Prime with a rejection so we can verify it's cleared on accept.
        consume_segment(&slot, 1_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        assert_eq!(
            snapshot(&slot)
                .await
                .unwrap()
                .last_rejected_reason
                .as_deref(),
            Some("too_short")
        );

        let out = consume_segment(&slot, 5_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        assert!(out.is_some());

        let snap = snapshot(&slot).await.unwrap();
        assert_eq!(snap.captured, 1);
        assert_eq!(snap.last_captured_duration_ms, Some(5_000.0));
        assert!(snap.last_rejected_reason.is_none());
        assert_eq!(snap.saved_embedding_ids.len(), 1);
        assert_eq!(snap.status, Status::Active);
    }

    #[tokio::test]
    async fn consume_segment_target_reached_flips_to_complete_and_clears_staging() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        start(&slot, speaker_id, 2).await.unwrap();

        consume_segment(&slot, 5_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        consume_segment(&slot, 6_000.0, 0.85, &pool, &good_embedding())
            .await
            .unwrap();

        let snap = snapshot(&slot).await.unwrap();
        assert_eq!(snap.captured, 2);
        assert_eq!(snap.status, Status::Complete);
        // Staging list cleared on completion so a stray cancel can't wipe
        // the legitimate voiceprint rows.
        assert!(snap.saved_embedding_ids.is_empty());
    }

    #[tokio::test]
    async fn cleanup_partial_embeddings_deletes_only_staging_ids() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        start(&slot, speaker_id, 3).await.unwrap();

        // Capture two segments mid-enrollment.
        consume_segment(&slot, 5_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        consume_segment(&slot, 5_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();

        // Pre-existing successful enrollment row for the same speaker
        // (e.g. from a prior session) — must survive the cleanup.
        let prior_id = crate::domain::speaker_matcher::save_embedding(
            &pool,
            speaker_id,
            &good_embedding(),
            5_000.0,
            0.9,
            true,
        )
        .await
        .unwrap();

        // User cancels.
        cancel(&slot).await;

        let removed = cleanup_partial_embeddings(&slot, &pool).await.unwrap();
        assert_eq!(removed, 2, "should remove the two staged rows only");

        // Prior row still present.
        let exists: Option<(String,)> =
            sqlx::query_as("SELECT id FROM speaker_embeddings WHERE id = ?1")
                .bind(prior_id.to_string())
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(
            exists.is_some(),
            "prior successful enrollment must survive cleanup"
        );
    }

    #[tokio::test]
    async fn cleanup_partial_embeddings_is_noop_after_complete() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        start(&slot, speaker_id, 1).await.unwrap();

        consume_segment(&slot, 5_000.0, 0.9, &pool, &good_embedding())
            .await
            .unwrap();
        // Status should now be Complete.
        let removed = cleanup_partial_embeddings(&slot, &pool).await.unwrap();
        assert_eq!(removed, 0, "completed sessions don't get their rows wiped");
    }

    #[tokio::test]
    async fn publish_level_does_not_bump_version() {
        let pool = fresh_pool().await;
        let speaker_id = mk_speaker(&pool).await;
        let slot = new_state();
        let initial = start(&slot, speaker_id, 3).await.unwrap();

        publish_level(&slot, 0.42);
        publish_level(&slot, 0.55);

        let snap = snapshot(&slot).await.unwrap();
        assert_eq!(
            snap.version, initial.version,
            "level updates must not spin the version counter"
        );
        assert!((snap.rms_level - 0.55).abs() < 1e-6);
    }
}
