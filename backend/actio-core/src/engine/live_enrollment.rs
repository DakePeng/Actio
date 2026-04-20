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
    /// Monotonic version that bumps on every mutation so the frontend can
    /// distinguish a fresh state from a stale cached one.
    pub version: u64,
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
///
/// Originally used by the enrollment UI's live-meter display (was showing
/// `rms_level` on `EnrollmentState`). That field is no longer part of the
/// serialised state, so this function is a no-op stub kept only so the
/// pipeline-level observer's call site remains valid. If the meter UX is
/// reintroduced, restore `rms_level` and write to it here.
pub fn publish_level(_slot: &LiveEnrollment, _rms: f32) {
    // intentionally empty
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
    if duration_ms < MIN_DURATION_MS
        || duration_ms > MAX_DURATION_MS
        || quality_score < MIN_QUALITY
    {
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
