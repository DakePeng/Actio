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
    /// A different speaker's tentative match we've seen once while `speaker_id`
    /// was still set. Requires a second consecutive tentative for the same
    /// candidate (or any confirmed evidence) to flip state — a single noisy
    /// segment (cross-talk, cough, laugh) no longer mis-attributes the next
    /// several segments.
    pub pending_tentative: Option<Uuid>,
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
                pending_tentative: None,
            },
        ),
        MatchEvidence::Tentative { speaker_id } => {
            // Tentative is positive-but-weak evidence. Rules:
            //   * No carry-over in progress → accept immediately (seed state).
            //   * Same speaker as current → refresh timer, clear any pending.
            //   * Different speaker, state stale (outside window) → accept;
            //     the carry-over would have dropped anyway.
            //   * Different speaker, state fresh, pending matches → flip
            //     (two consecutive tentatives for the new speaker).
            //   * Different speaker, state fresh, pending doesn't match →
            //     carry current; stash candidate as pending so a second
            //     tentative for the same new speaker promotes to flip.
            let current = state.speaker_id;
            let fresh = within_window(state, segment_end_ms, config);
            let same_current = current == Some(speaker_id);

            if current.is_none() || same_current || !fresh {
                return (
                    AttributionOutcome {
                        speaker_id: Some(speaker_id),
                        confidence: Some(MatchConfidence::Tentative),
                        carried_over: false,
                    },
                    ContinuityState {
                        speaker_id: Some(speaker_id),
                        last_confirmed_ms: Some(segment_end_ms),
                        pending_tentative: None,
                    },
                );
            }

            // Fresh carry-over for a different speaker. Require corroboration
            // before flipping.
            if state.pending_tentative == Some(speaker_id) {
                (
                    AttributionOutcome {
                        speaker_id: Some(speaker_id),
                        confidence: Some(MatchConfidence::Tentative),
                        carried_over: false,
                    },
                    ContinuityState {
                        speaker_id: Some(speaker_id),
                        last_confirmed_ms: Some(segment_end_ms),
                        pending_tentative: None,
                    },
                )
            } else {
                (
                    AttributionOutcome {
                        speaker_id: current,
                        confidence: Some(MatchConfidence::Tentative),
                        carried_over: true,
                    },
                    ContinuityState {
                        speaker_id: current,
                        last_confirmed_ms: state.last_confirmed_ms,
                        pending_tentative: Some(speaker_id),
                    },
                )
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
    // Reject out-of-order segments explicitly instead of letting `now - t`
    // underflow into a negative number that trivially passes the <= test.
    // In-order delivery is the caller's responsibility, but the cached
    // embedding-worker pool can in theory resolve two segments for the same
    // session out of order — defend against it here rather than trust the
    // upstream invariant.
    state.last_confirmed_ms.map_or(false, |t| {
        if now < t {
            return false;
        }
        now.saturating_sub(t) <= config.window_ms as i64
    })
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
    fn uuid_c() -> Uuid {
        Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap()
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
            pending_tentative: None,
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
        assert_eq!(new_state.pending_tentative, None);
    }

    #[test]
    fn confirmed_clears_pending_tentative() {
        let a = uuid_a();
        let b = uuid_b();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: Some(b),
        };
        let (_, new_state) = next_attribution(
            &state,
            3_000,
            MatchEvidence::Confirmed { speaker_id: a },
            WINDOW,
        );
        assert_eq!(new_state.pending_tentative, None);
    }

    #[test]
    fn same_speaker_tentative_refreshes_timer() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: None,
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
    fn single_tentative_for_different_speaker_does_not_flip() {
        // A single tentative match for a different speaker (noisy segment,
        // cross-talk, cough that happens to score tentative against someone
        // else) carries current and stashes the candidate as pending.
        let a = uuid_a();
        let b = uuid_b();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: None,
        };
        let (outcome, new_state) = next_attribution(
            &state,
            5_000,
            MatchEvidence::Tentative { speaker_id: b },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(a));
        assert_eq!(outcome.confidence, Some(MatchConfidence::Tentative));
        assert!(outcome.carried_over);
        assert_eq!(new_state.speaker_id, Some(a));
        assert_eq!(new_state.last_confirmed_ms, Some(0));
        assert_eq!(new_state.pending_tentative, Some(b));
    }

    #[test]
    fn two_consecutive_tentatives_flip_state() {
        // Speaker B really has started talking — two consecutive tentative
        // matches for B promote to a flip even without a confirmed.
        let a = uuid_a();
        let b = uuid_b();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: None,
        };
        let (_, after_first) = next_attribution(
            &state,
            5_000,
            MatchEvidence::Tentative { speaker_id: b },
            WINDOW,
        );
        let (outcome, new_state) = next_attribution(
            &after_first,
            7_000,
            MatchEvidence::Tentative { speaker_id: b },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(b));
        assert_eq!(outcome.confidence, Some(MatchConfidence::Tentative));
        assert!(!outcome.carried_over);
        assert_eq!(new_state.speaker_id, Some(b));
        assert_eq!(new_state.last_confirmed_ms, Some(7_000));
        assert_eq!(new_state.pending_tentative, None);
    }

    #[test]
    fn tentative_for_different_speaker_outside_window_flips_immediately() {
        // The carry-over window has expired, so there's nothing to defend —
        // accept the new speaker right away instead of requiring a second.
        let a = uuid_a();
        let b = uuid_b();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: None,
        };
        let (outcome, new_state) = next_attribution(
            &state,
            20_000, // outside the 15_000ms window
            MatchEvidence::Tentative { speaker_id: b },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(b));
        assert!(!outcome.carried_over);
        assert_eq!(new_state.speaker_id, Some(b));
    }

    #[test]
    fn pending_candidate_is_replaced_by_newer_candidate() {
        // If we see tentative(B) then tentative(C) — neither gets promoted.
        // Pending tracks only the most-recently-seen candidate.
        let a = uuid_a();
        let b = uuid_b();
        let c = uuid_c();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: Some(b),
        };
        let (outcome, new_state) = next_attribution(
            &state,
            5_000,
            MatchEvidence::Tentative { speaker_id: c },
            WINDOW,
        );
        assert_eq!(outcome.speaker_id, Some(a));
        assert!(outcome.carried_over);
        assert_eq!(new_state.pending_tentative, Some(c));
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
        assert_eq!(new_state.speaker_id, Some(b));
        assert_eq!(new_state.last_confirmed_ms, Some(7_000));
    }

    #[test]
    fn unknown_carries_over_within_window() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: None,
        };
        let (outcome, new_state) = next_attribution(&state, 10_000, MatchEvidence::Unknown, WINDOW);
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
            pending_tentative: None,
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
            pending_tentative: None,
        };
        // First Unknown carries (10_000 within 15_000 window).
        let (carry, after_carry) = next_attribution(&state, 10_000, MatchEvidence::Unknown, WINDOW);
        assert!(carry.carried_over);
        assert_eq!(after_carry, state, "carry must leave state alone");

        // Second Unknown at 25_000 — still only 15_000 from the original
        // last_confirmed_ms of 0, so outside window → drops.
        let (drop, _) = next_attribution(&after_carry, 25_000, MatchEvidence::Unknown, WINDOW);
        assert!(drop.speaker_id.is_none());
        assert!(!drop.carried_over);
    }

    #[test]
    fn window_zero_disables_carry_over() {
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(0),
            pending_tentative: None,
        };
        let off = ContinuityConfig { window_ms: 0 };
        let (outcome, _) = next_attribution(&state, 1, MatchEvidence::Unknown, off);
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

    #[test]
    fn out_of_order_segment_does_not_trivially_carry() {
        // If a segment resolves with end_ms earlier than last_confirmed_ms
        // (which the cached worker pool now makes structurally possible),
        // within_window must reject rather than treat the negative delta
        // as passing the <= test.
        let a = uuid_a();
        let state = ContinuityState {
            speaker_id: Some(a),
            last_confirmed_ms: Some(10_000),
            pending_tentative: None,
        };
        let (outcome, _) = next_attribution(
            &state,
            5_000, // earlier than last_confirmed_ms
            MatchEvidence::Unknown,
            WINDOW,
        );
        assert!(outcome.speaker_id.is_none(), "out-of-order must not carry");
        assert!(!outcome.carried_over);
    }
}
