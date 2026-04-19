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
/// See the design spec's decision table for the full set of invariants.
/// Task 4 fills in the Unknown carry-over arm; Confirmed and Tentative
/// are already implemented.
pub fn next_attribution(
    state: &ContinuityState,
    segment_end_ms: i64,
    evidence: MatchEvidence,
    config: ContinuityConfig,
) -> (AttributionOutcome, ContinuityState) {
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
        MatchEvidence::Tentative { speaker_id } => {
            let within = within_window(state, segment_end_ms, config);
            match state.speaker_id {
                Some(active) if within && active == speaker_id => (
                    AttributionOutcome {
                        speaker_id: Some(speaker_id),
                        confidence: Some(MatchConfidence::Tentative),
                        carried_over: false,
                    },
                    ContinuityState {
                        speaker_id: Some(speaker_id),
                        last_confirmed_ms: Some(segment_end_ms),
                    },
                ),
                Some(active) if within => (
                    AttributionOutcome {
                        speaker_id: Some(active),
                        confidence: Some(MatchConfidence::Tentative),
                        carried_over: true,
                    },
                    *state,
                ),
                // No live state (either never seeded or window expired):
                // accept the Tentative speaker but do not seed state.
                _ => (
                    AttributionOutcome {
                        speaker_id: Some(speaker_id),
                        confidence: Some(MatchConfidence::Tentative),
                        carried_over: false,
                    },
                    *state,
                ),
            }
        }
        MatchEvidence::Unknown => {
            if within_window(state, segment_end_ms, config) {
                (
                    AttributionOutcome {
                        speaker_id: state.speaker_id,
                        confidence: Some(MatchConfidence::Tentative),
                        carried_over: true,
                    },
                    *state,
                )
            } else {
                (
                    AttributionOutcome {
                        speaker_id: None,
                        confidence: None,
                        carried_over: false,
                    },
                    *state,
                )
            }
        }
    }
}

fn within_window(state: &ContinuityState, now: i64, config: ContinuityConfig) -> bool {
    if config.window_ms == 0 {
        return false;
    }
    // If `now < t` (segment received out of order), `now - t` is negative
    // and the comparison is trivially true; in-order processing is the
    // caller's responsibility (enforced by the VAD consumer loop).
    state
        .last_confirmed_ms
        .map_or(false, |t| now - t <= config.window_ms as i64)
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

    #[test]
    fn same_speaker_tentative_refreshes_timer() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
        };
        let (outcome, new_state) = next_attribution(
            &state,
            14_000,
            MatchEvidence::Tentative { speaker_id: a },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(a));
        assert_eq!(outcome.confidence, Some(MatchConfidence::Tentative));
        assert!(!outcome.carried_over);
        assert_eq!(new_state.speaker_id, Some(a));
        assert_eq!(
            new_state.last_confirmed_ms,
            Some(14_000),
            "same-speaker tentative within window must refresh the timer"
        );
    }

    #[test]
    fn different_speaker_tentative_ignored_when_state_live() {
        let a = uuid_a();
        let b = uuid_b();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
        };
        let (outcome, new_state) = next_attribution(
            &state,
            5_000,
            MatchEvidence::Tentative { speaker_id: b },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(a), "A should stay attributed");
        assert_eq!(outcome.confidence, Some(MatchConfidence::Tentative));
        assert!(outcome.carried_over);
        assert_eq!(new_state, state, "weak contrary evidence must not update state");
    }

    #[test]
    fn different_speaker_tentative_accepted_without_state() {
        let b = uuid_b();
        let (outcome, new_state) = next_attribution(
            &ContinuityState::default(),
            7_000,
            MatchEvidence::Tentative { speaker_id: b },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(b));
        assert_eq!(outcome.confidence, Some(MatchConfidence::Tentative));
        assert!(!outcome.carried_over);
        assert_eq!(
            new_state,
            ContinuityState::default(),
            "Tentative alone must not seed state"
        );
    }

    #[test]
    fn unknown_carries_over_within_window() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
        };
        let (outcome, new_state) = next_attribution(
            &state,
            10_000,
            MatchEvidence::Unknown,
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(a));
        assert_eq!(outcome.confidence, Some(MatchConfidence::Tentative));
        assert!(outcome.carried_over);
        assert_eq!(new_state, state, "carry-over must not update state");
    }

    #[test]
    fn unknown_outside_window_drops() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
        };
        let (outcome, new_state) = next_attribution(
            &state,
            20_000, // window is 15_000, so 20_000 is outside
            MatchEvidence::Unknown,
            WINDOW,
        );
        assert!(outcome.speaker_id.is_none());
        assert!(outcome.confidence.is_none());
        assert!(!outcome.carried_over);
        assert_eq!(new_state, state);
    }

    #[test]
    fn carry_over_does_not_self_extend_the_window() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
        };
        // First Unknown carries (10_000 within 15_000 window).
        let (carry, after_carry) =
            next_attribution(&state, 10_000, MatchEvidence::Unknown, WINDOW);
        assert!(carry.carried_over);
        assert_eq!(after_carry, state, "carry must leave state alone");

        // Second Unknown at 25_000 — still only 15_000 from the original
        // last_confirmed_ms of 0, so outside window → drops.
        let (drop, _) =
            next_attribution(&after_carry, 25_000, MatchEvidence::Unknown, WINDOW);
        assert!(drop.speaker_id.is_none());
        assert!(!drop.carried_over);
    }

    #[test]
    fn window_zero_disables_carry_over() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
        };
        let off = ContinuityConfig { window_ms: 0 };
        let (outcome, _) =
            next_attribution(&state, 1, MatchEvidence::Unknown, off);
        assert!(outcome.speaker_id.is_none());
        assert!(!outcome.carried_over);

        // Confirmed and Tentative paths still behave normally with window=0.
        let b = uuid_b();
        let (c_out, _) = next_attribution(
            &ContinuityState::default(),
            1,
            MatchEvidence::Confirmed { speaker_id: b },
            off,
        );
        assert_eq!(c_out.speaker_id, Some(b));
    }
}
