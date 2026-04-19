//! Speaker continuity state machine — turns per-segment match evidence
//! into an attribution outcome that inherits from a recent confirmed
//! speaker when the current segment's own evidence is weak or absent.
//!
//! Pure function + plain data. No I/O, no async, no pool. See
//! `docs/superpowers/specs/2026-04-18-speaker-continuity-design.md` for
//! the decision table and invariants.

use uuid::Uuid;

use crate::domain::speaker_matcher::MatchConfidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContinuityState {
    pub speaker_id: Option<Uuid>,
    /// Session-relative timestamp (ms since session start) of the most
    /// recent event that seeded or refreshed state. Uses segment time, not
    /// processing time, so out-of-order task completion cannot corrupt it.
    pub last_confirmed_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuityConfig {
    /// 0 disables carry-over entirely. In practice clamped to [0, 60000]
    /// by the settings layer; the state machine treats any value as-is.
    pub window_ms: u32,
}

/// State-machine input. Closed enum so the caller converts each path
/// (matcher result, too-short, below-threshold, error) into exactly one
/// canonical variant before handing off to `next_attribution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEvidence {
    Confirmed { speaker_id: Uuid },
    Tentative { speaker_id: Uuid },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributionOutcome {
    pub speaker_id: Option<Uuid>,
    pub confidence: Option<MatchConfidence>,
    pub carried_over: bool,
}

/// Apply the state machine. Returns `(outcome_for_this_segment, new_state)`.
/// Caller replaces the existing state with the returned `new_state` whether
/// or not `outcome.carried_over` is true; carry-over outcomes return the
/// state unchanged (no self-reinforcement).
///
/// Task 1 ships a stub; Tasks 2–4 implement the three evidence arms.
pub fn next_attribution(
    state: &ContinuityState,
    segment_end_ms: i64,
    evidence: MatchEvidence,
    config: ContinuityConfig,
) -> (AttributionOutcome, ContinuityState) {
    let _ = config;
    match evidence {
        MatchEvidence::Confirmed { speaker_id } => (
            AttributionOutcome {
                speaker_id: Some(speaker_id),
                confidence: Some(MatchConfidence::Confirmed),
                carried_over: false,
            },
            ContinuityState {
                speaker_id: Some(speaker_id),
                last_confirmed_ms: Some(segment_end_ms),
            },
        ),
        MatchEvidence::Tentative { .. } | MatchEvidence::Unknown => (
            AttributionOutcome {
                speaker_id: None,
                confidence: None,
                carried_over: false,
            },
            *state,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: ContinuityConfig = ContinuityConfig { window_ms: 15_000 };

    fn uuid_a() -> Uuid {
        Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
    }
    fn uuid_b() -> Uuid {
        Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap()
    }

    #[test]
    fn confirmed_seeds_state_from_empty() {
        let a = uuid_a();
        let (outcome, new_state) = next_attribution(
            &ContinuityState::default(),
            0,
            MatchEvidence::Confirmed { speaker_id: a },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(a));
        assert_eq!(outcome.confidence, Some(MatchConfidence::Confirmed));
        assert!(!outcome.carried_over);
        assert_eq!(new_state.speaker_id, Some(a));
        assert_eq!(new_state.last_confirmed_ms, Some(0));
    }

    #[test]
    fn confirmed_flips_to_another_speaker() {
        let a = uuid_a();
        let b = uuid_b();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
        };
        let (outcome, new_state) = next_attribution(
            &state,
            5_000,
            MatchEvidence::Confirmed { speaker_id: b },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(b));
        assert_eq!(outcome.confidence, Some(MatchConfidence::Confirmed));
        assert!(!outcome.carried_over);
        assert_eq!(new_state.speaker_id, Some(b));
        assert_eq!(new_state.last_confirmed_ms, Some(5_000));
    }
}
