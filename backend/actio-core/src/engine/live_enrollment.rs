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

pub async fn snapshot(slot: &LiveEnrollment) -> Option<EnrollmentState> {
    slot.lock().await.clone()
}

/// Publish a smoothed RMS level sampled from the audio capture tap.
/// Cheap hot-path — try-lock only, no version bump, no-op when no
/// enrollment is armed.
pub fn publish_level(slot: &LiveEnrollment, rms: f32) {
    let Ok(mut guard) = slot.try_lock() else { return };
    if let Some(state) = guard.as_mut() {
        if state.status == Status::Active {
            state.rms_level = rms;
        }
    }
}

async fn mark_rejected(slot: &LiveEnrollment, reason: &'static str) {
    let mut guard = slot.lock().await;
    if let Some(state) = guard.as_mut() {
        if state.status == Status::Active {
            state.last_rejected_reason = Some(reason.to_string());
            state.version += 1;
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

    // Quick unlocked check first to avoid hot-path contention when idle.
    let Some(snap) = snapshot(slot).await else {
        return Ok(None);
    };
    if snap.status != Status::Active {
        return Ok(None);
    }
    if duration_ms < MIN_DURATION_MS {
        mark_rejected(slot, "too_short").await;
        return Ok(None);
    }
    if duration_ms > MAX_DURATION_MS {
        mark_rejected(slot, "too_long").await;
        return Ok(None);
    }
    if quality_score < MIN_QUALITY {
        mark_rejected(slot, "low_quality").await;
        return Ok(None);
    }

    // Re-check under the lock and claim this segment.
    let (speaker_uuid, is_first, target_reached) = {
        let mut guard = slot.lock().await;
        let Some(state) = guard.as_mut() else {
            return Ok(None);
        };
        if state.status != Status::Active {
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

    // Persist as a speaker_embeddings row. First capture becomes primary.
    crate::domain::speaker_matcher::save_embedding(
        pool,
        speaker_uuid,
        embedding,
        duration_ms,
        quality_score as f64,
        is_first,
    )
    .await?;

    let _ = target_reached; // already reflected in state
    Ok(Some(speaker_uuid.to_string()))
}
