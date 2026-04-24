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
