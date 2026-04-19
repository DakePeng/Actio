# Speaker Continuity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Inherit a recent confirmed speaker's attribution when a subsequent VAD segment's match is weak or absent, so one speech turn renders coherently under one speaker instead of flickering between Confirmed / Tentative / Unknown.

**Architecture:** New pure `continuity` module exposes a state machine `next_attribution(state, segment_end_ms, evidence, config) -> (outcome, new_state)`. The pipeline builds `MatchEvidence` from matcher output and short / error paths, feeds it through the state machine inside a new `finalize_segment` helper, and persists + publishes the resulting `AttributionOutcome`. Per-segment work switches from `tokio::spawn` to inline in the VAD consumer loop so the state machine sees segments strictly in VAD order; the offline ASR path forwards to its receiver *before* awaiting the speaker hook so transcription latency is unaffected. State is session-scoped (`Arc<Mutex<_>>` created inside each `start_session`; dropped naturally on restart), not on `AppState`.

**Tech Stack:** Rust 2021 (`sqlx`, `tokio`, `serde`, `uuid`, `bytemuck`, `anyhow`, `tracing`), React 19 + TypeScript + Vitest.

**Related spec:** `docs/superpowers/specs/2026-04-18-speaker-continuity-design.md`.

**Touched files:**
- Create: `backend/actio-core/src/engine/continuity.rs` — pure state machine module.
- Modify: `backend/actio-core/src/engine/mod.rs` — register module.
- Modify: `backend/actio-core/src/engine/app_settings.rs` — new `speaker_continuity_window_ms` field + patch.
- Modify: `backend/actio-core/src/engine/inference_pipeline.rs` — `SpeakerIdConfig.continuity_window_ms`, `finalize_segment` helper, inline forward-first processing, continuity Arc plumbing, `MatchEvidence` construction, integration test.
- Modify: `backend/actio-core/src/engine/transcript_aggregator.rs` — `carried_over: bool` on `SpeakerResolvedEvent`.
- Modify: `backend/actio-core/src/api/ws.rs` — `WsSpeakerResolvedEvent.carried_over` + `is_false` helper.
- Modify: `backend/actio-core/src/api/settings.rs` — expanded restart-comparison tuple.
- Modify: `backend/actio-core/src/lib.rs` — always-on start_session gets `continuity_window_ms`.
- Modify: `backend/actio-core/src/api/session.rs` — create-session and enrollment start_session calls get `continuity_window_ms`.
- Modify: `frontend/src/components/settings/AudioSettings.tsx` — new Continuity window slider.

---

### Task 1: Create continuity module skeleton

**Files:**
- Create: `backend/actio-core/src/engine/continuity.rs`
- Modify: `backend/actio-core/src/engine/mod.rs`

- [ ] **Step 1: Create the continuity module with types and a stub function**

Write `backend/actio-core/src/engine/continuity.rs`:

```rust
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

#[derive(Debug, Clone, Copy)]
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
    let _ = (segment_end_ms, evidence, config);
    (
        AttributionOutcome {
            speaker_id: None,
            confidence: None,
            carried_over: false,
        },
        *state,
    )
}
```

- [ ] **Step 2: Register the new module**

Modify `backend/actio-core/src/engine/mod.rs`: add `pub mod continuity;` in the same block as the other `pub mod` declarations (alphabetic among engine submodules).

- [ ] **Step 3: Verify it compiles**

Run: `cd /d/Dev/Actio/backend && cargo check -p actio-core`
Expected: `Finished ... dev [unoptimized + debuginfo]` with no errors.

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/continuity.rs backend/actio-core/src/engine/mod.rs
git commit -m "feat(continuity): scaffold module with types and stub next_attribution"
```

---

### Task 2: `next_attribution` — Confirmed evidence (TDD)

**Files:**
- Modify: `backend/actio-core/src/engine/continuity.rs` (add `#[cfg(test)] mod tests` and extend `next_attribution`)

- [ ] **Step 1: Write the failing test for "Confirmed seeds state"**

Append to `backend/actio-core/src/engine/continuity.rs`:

```rust
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
}
```

- [ ] **Step 2: Run the test — it should fail**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests::confirmed_seeds_state_from_empty`
Expected: FAIL — assertion on `outcome.speaker_id` fails because the stub returns `None`.

- [ ] **Step 3: Implement the Confirmed arm**

Replace the body of `next_attribution` in `backend/actio-core/src/engine/continuity.rs`:

```rust
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
```

- [ ] **Step 4: Run the test — it should pass**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests::confirmed_seeds_state_from_empty`
Expected: PASS.

- [ ] **Step 5: Add the "Confirmed flips" test**

Append inside `mod tests { ... }` in `backend/actio-core/src/engine/continuity.rs`:

```rust
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
```

- [ ] **Step 6: Run both tests — both should pass**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests`
Expected: 2 passed.

- [ ] **Step 7: Commit**

```bash
git add backend/actio-core/src/engine/continuity.rs
git commit -m "feat(continuity): implement Confirmed arm with seed-and-flip tests"
```

---

### Task 3: `next_attribution` — Tentative evidence (TDD)

**Files:**
- Modify: `backend/actio-core/src/engine/continuity.rs`

- [ ] **Step 1: Add the `within_window` helper stub and first Tentative test**

Append inside `mod tests { ... }`:

```rust
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
```

- [ ] **Step 2: Run the test — it should fail**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests::same_speaker_tentative_refreshes_timer`
Expected: FAIL — `outcome.speaker_id` is `None`, new_state.last_confirmed_ms is still `Some(0)`.

- [ ] **Step 3: Implement the Tentative arm and the `within_window` helper**

Replace the body of `next_attribution` in `backend/actio-core/src/engine/continuity.rs`:

```rust
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
        MatchEvidence::Unknown => (
            AttributionOutcome {
                speaker_id: None,
                confidence: None,
                carried_over: false,
            },
            *state,
        ),
    }
}

fn within_window(state: &ContinuityState, now: i64, config: ContinuityConfig) -> bool {
    if config.window_ms == 0 {
        return false;
    }
    state
        .last_confirmed_ms
        .map_or(false, |t| now - t <= config.window_ms as i64)
}
```

- [ ] **Step 4: Run the test — it should pass**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests::same_speaker_tentative_refreshes_timer`
Expected: PASS.

- [ ] **Step 5: Add "different-speaker Tentative ignored when state live" test**

Append inside `mod tests { ... }`:

```rust
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
```

- [ ] **Step 6: Add "different-speaker Tentative with no state" test**

Append inside `mod tests { ... }`:

```rust
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
```

- [ ] **Step 7: Run Tentative tests — all should pass**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests`
Expected: 4 passed (two Confirmed + two new Tentative; Step 5's fires here too).

Wait for all five total (including the one from Step 1). Re-run if count is off.

- [ ] **Step 8: Commit**

```bash
git add backend/actio-core/src/engine/continuity.rs
git commit -m "feat(continuity): implement Tentative arm (refresh / ignore / accept)"
```

---

### Task 4: `next_attribution` — Unknown / carry-over (TDD)

**Files:**
- Modify: `backend/actio-core/src/engine/continuity.rs`

- [ ] **Step 1: Add "Unknown carries over within window" test**

Append inside `mod tests { ... }`:

```rust
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
```

- [ ] **Step 2: Run the test — it should fail**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests::unknown_carries_over_within_window`
Expected: FAIL — current Unknown arm always returns `(None, None, false)`.

- [ ] **Step 3: Implement Unknown carry-over**

Replace the `MatchEvidence::Unknown =>` arm in `next_attribution` (in `backend/actio-core/src/engine/continuity.rs`) with:

```rust
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
```

- [ ] **Step 4: Run the test — it should pass**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests::unknown_carries_over_within_window`
Expected: PASS.

- [ ] **Step 5: Add the remaining Unknown tests**

Append inside `mod tests { ... }`:

```rust
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
```

- [ ] **Step 6: Run the full continuity test module**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core continuity::tests`
Expected: 8 passed (2 Confirmed + 3 Tentative + 3 Unknown).

- [ ] **Step 7: Run the full backend test suite to confirm no regression**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core`
Expected: all previous tests still pass (the current baseline is 74 passed). New total 82 passed.

- [ ] **Step 8: Commit**

```bash
git add backend/actio-core/src/engine/continuity.rs
git commit -m "feat(continuity): implement Unknown arm with carry-over + full coverage"
```

---

### Task 5: Add `speaker_continuity_window_ms` to `AudioSettings`

**Files:**
- Modify: `backend/actio-core/src/engine/app_settings.rs`

- [ ] **Step 1: Add the field to `AudioSettings`**

In `backend/actio-core/src/engine/app_settings.rs`, extend the `AudioSettings` struct. Find the block that starts with `pub struct AudioSettings {` and add this new field after `speaker_min_duration_ms`:

```rust
    /// Milliseconds of time-decay window for the continuity state machine.
    /// When a Confirmed match is received, subsequent Unknown / weak
    /// segments within this window inherit that speaker. 0 disables
    /// carry-over entirely. Clamped to [0, 60000] on patch.
    #[serde(default = "default_speaker_continuity_window_ms")]
    pub speaker_continuity_window_ms: u32,
```

- [ ] **Step 2: Add the default function**

Add below the existing `default_speaker_min_duration_ms` function:

```rust
fn default_speaker_continuity_window_ms() -> u32 {
    15_000
}
```

- [ ] **Step 3: Update `AudioSettings::default()`**

In `impl Default for AudioSettings`, add the field to the returned struct:

```rust
            speaker_continuity_window_ms: default_speaker_continuity_window_ms(),
```

- [ ] **Step 4: Extend `AudioSettingsPatch`**

Find the `pub struct AudioSettingsPatch { ... }` block. Add:

```rust
    pub speaker_continuity_window_ms: Option<u32>,
```

- [ ] **Step 5: Apply the patch in `SettingsManager::update`**

Find the `if let Some(audio) = patch.audio {` block. Below the existing `if let Some(v) = audio.speaker_min_duration_ms { ... }` clause, add:

```rust
            if let Some(v) = audio.speaker_continuity_window_ms {
                settings.audio.speaker_continuity_window_ms = v.min(60_000);
            }
```

- [ ] **Step 6: Verify compilation + existing tests**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core app_settings`
Expected: all `app_settings::tests` pass; field is serialized with defaults for legacy `settings.json` files.

- [ ] **Step 7: Commit**

```bash
git add backend/actio-core/src/engine/app_settings.rs
git commit -m "feat(settings): add speaker_continuity_window_ms to AudioSettings"
```

---

### Task 6: Add `continuity_window_ms` to `SpeakerIdConfig` and thread it through start_session call sites

**Files:**
- Modify: `backend/actio-core/src/engine/inference_pipeline.rs`
- Modify: `backend/actio-core/src/lib.rs`
- Modify: `backend/actio-core/src/api/session.rs`

- [ ] **Step 1: Extend `SpeakerIdConfig`**

In `backend/actio-core/src/engine/inference_pipeline.rs`, modify the `SpeakerIdConfig` struct and its `Default` impl:

```rust
#[derive(Debug, Clone, Copy)]
pub struct SpeakerIdConfig {
    pub confirm_threshold: f32,
    pub tentative_threshold: f32,
    pub min_duration_ms: u32,
    pub continuity_window_ms: u32,
}

impl Default for SpeakerIdConfig {
    fn default() -> Self {
        Self {
            confirm_threshold: 0.55,
            tentative_threshold: 0.40,
            min_duration_ms: 1500,
            continuity_window_ms: 15_000,
        }
    }
}
```

- [ ] **Step 2: Populate the field at the always-on start_session call**

In `backend/actio-core/src/lib.rs`, find `start_always_on_pipeline` and the `SpeakerIdConfig { ... }` literal inside the `pipeline.start_session(...)` call. Add the field:

```rust
            crate::engine::inference_pipeline::SpeakerIdConfig {
                confirm_threshold: settings.audio.speaker_confirm_threshold,
                tentative_threshold: settings.audio.speaker_tentative_threshold,
                min_duration_ms: settings.audio.speaker_min_duration_ms,
                continuity_window_ms: settings.audio.speaker_continuity_window_ms,
            },
```

- [ ] **Step 3: Populate the field at the transcription session start**

In `backend/actio-core/src/api/session.rs`, find `pub async fn create_session` and the `SpeakerIdConfig { ... }` literal inside `pipeline.start_session(...)`. Add the same `continuity_window_ms` line.

- [ ] **Step 4: Populate the field at the enrollment session start**

Still in `backend/actio-core/src/api/session.rs`, find `pub async fn start_live_enrollment`. Inside the `pipeline.start_session(...)` call's `SpeakerIdConfig { ... }` literal, add `continuity_window_ms: settings.audio.speaker_continuity_window_ms,`.

- [ ] **Step 5: Verify compilation**

Run: `cd /d/Dev/Actio/backend && cargo check -p actio-core`
Expected: compiles with no errors. The field isn't used yet in `handle_segment_embedding` — that comes in Task 8.

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/engine/inference_pipeline.rs backend/actio-core/src/lib.rs backend/actio-core/src/api/session.rs
git commit -m "feat(pipeline): thread continuity_window_ms through SpeakerIdConfig"
```

---

### Task 7: Add `carried_over` to `SpeakerResolvedEvent` and `WsSpeakerResolvedEvent`

**Files:**
- Modify: `backend/actio-core/src/engine/transcript_aggregator.rs`
- Modify: `backend/actio-core/src/engine/inference_pipeline.rs` (update existing publish closure + callers)
- Modify: `backend/actio-core/src/api/ws.rs`

- [ ] **Step 1: Add the field to `SpeakerResolvedEvent`**

In `backend/actio-core/src/engine/transcript_aggregator.rs`, find `pub struct SpeakerResolvedEvent {` and add:

```rust
    /// True when this attribution was inherited from an earlier Confirmed
    /// match rather than produced by the current segment's own evidence.
    /// Clients may render it differently for a v2 polish pass.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub carried_over: bool,
```

Note: the `std::ops::Not::not` attribute works here because `carried_over` is owned (not a reference) and serde's built-in support for `Not::not` on `bool` is the recommended pattern for field-level `skip_serializing_if`. If `cargo check` later rejects it, replace with a local `fn` — see Task 7 Step 5.

Actually, don't guess. Ship it with a helper from the start; it's the codex-validated form. Replace the attribute above with:

```rust
    #[serde(default, skip_serializing_if = "crate::engine::transcript_aggregator::is_false")]
    pub carried_over: bool,
```

And at the top of `transcript_aggregator.rs` (near other top-level items), add:

```rust
/// Serde helper so `carried_over: false` is omitted from the wire format.
pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}
```

- [ ] **Step 2: Update the publish closure in `handle_segment_embedding`**

In `backend/actio-core/src/engine/inference_pipeline.rs`, find the `let publish = |speaker_id: Option<String>, confidence: Option<&'static str>|` closure inside `async fn handle_segment_embedding`. Change it to:

```rust
    let publish = |speaker_id: Option<String>, confidence: Option<&'static str>, carried_over: bool| {
        aggregator.publish_speaker_resolved(
            crate::engine::transcript_aggregator::SpeakerResolvedEvent {
                segment_id: segment_id.to_string(),
                start_ms,
                end_ms,
                speaker_id,
                confidence,
                carried_over,
            },
        );
    };
```

- [ ] **Step 3: Update every `publish(...)` call inside the function**

Still inside `handle_segment_embedding`, locate each `publish(...)` call and append `, false`:

- The "no embedding model" branch: `publish(None, None, false);`
- The "embedding extraction failed" branch: `publish(None, None, false);`
- The live-enrollment Confirmed branch: `publish(Some(enrolled_speaker.clone()), Some(crate::domain::speaker_matcher::MatchConfidence::Confirmed.as_str()), false);`
- The too-short branch: `publish(None, None, false);`
- The final normal-identify branch: `publish(result.speaker_id.clone(), result.confidence.map(|c| c.as_str()), false);`

Task 8 will re-wire the identify and too-short branches to use `finalize_segment` which will compute `carried_over` properly. For now all callers pass `false`, preserving today's behavior.

- [ ] **Step 4: Extend `WsSpeakerResolvedEvent`**

In `backend/actio-core/src/api/ws.rs`, first add the helper near the top of the file (just after the `use` imports, or next to the other local helpers):

```rust
fn is_false(b: &bool) -> bool {
    !*b
}
```

Then find the `WsSpeakerResolvedEvent` struct and add the field:

```rust
#[derive(Serialize)]
struct WsSpeakerResolvedEvent {
    kind: &'static str,
    segment_id: String,
    start_ms: i64,
    end_ms: i64,
    speaker_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<&'static str>,
    #[serde(skip_serializing_if = "is_false")]
    carried_over: bool,
}
```

Find the construction site (look for `WsSpeakerResolvedEvent {` inside `handle_socket`) and add:

```rust
                                carried_over: sr.carried_over,
```

- [ ] **Step 5: Verify compilation and run the existing test suite**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core`
Expected: same 82 passed as end of Task 4. Nothing new in behavior; the field is false everywhere and serialized-out when false.

- [ ] **Step 6: Commit**

```bash
git add backend/actio-core/src/engine/transcript_aggregator.rs backend/actio-core/src/engine/inference_pipeline.rs backend/actio-core/src/api/ws.rs
git commit -m "feat(ws): add carried_over flag to SpeakerResolvedEvent"
```

---

### Task 8: Extract `finalize_segment`, switch to inline forward-first processing

This is the biggest behavior change. It:
- extracts a testable `finalize_segment` helper that owns continuity state, persistence, and publish,
- converts `MatchEvidence` from matcher results / early-return paths,
- replaces `tokio::spawn`-per-segment with inline sequential processing in the VAD consumer loop,
- forwards to offline ASR before awaiting the speaker hook so transcription latency is unchanged,
- creates the session-scoped continuity `Arc<Mutex<_>>` inside `start_session`.

**Files:**
- Modify: `backend/actio-core/src/engine/inference_pipeline.rs`

- [ ] **Step 1: Add imports for continuity types**

At the top of `backend/actio-core/src/engine/inference_pipeline.rs`, with the other `use crate::engine::...` lines, add:

```rust
use crate::engine::continuity::{
    self, AttributionOutcome, ContinuityConfig, ContinuityState, MatchEvidence,
};
```

Also ensure `tokio::sync::Mutex` is available. Add under the existing `use tokio::sync::mpsc;`:

```rust
use tokio::sync::Mutex;
```

- [ ] **Step 2: Add the `finalize_segment` helper**

Inside `backend/actio-core/src/engine/inference_pipeline.rs`, below the existing `async fn handle_segment_embedding` (after its closing brace), add the new helper:

```rust
/// Shared tail for every path that wants to participate in continuity:
/// too-short, below-threshold, Tentative, and Confirmed. Runs the state
/// machine, persists the segment row with the outcome's speaker_id, and
/// publishes the corresponding WS event.
///
/// `match_similarity` is `Some` only for paths whose `evidence` came from
/// `identify_speaker_with_thresholds` and where `carried_over == false`.
/// Carry-over / too-short / matcher-error paths persist a NULL
/// `speaker_score` so downstream consumers don't see a fabricated value.
#[allow(clippy::too_many_arguments)]
async fn finalize_segment(
    pool: &sqlx::SqlitePool,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    embedding: Option<&[f32]>,
    audio: &[f32],
    audio_quality: f32,
    clips_dir: &Path,
    evidence: MatchEvidence,
    match_similarity: Option<f64>,
    continuity: &Arc<Mutex<ContinuityState>>,
    config: ContinuityConfig,
    aggregator: &Arc<TranscriptAggregator>,
    segment_id: Uuid,
) -> anyhow::Result<Option<String>> {
    // Run the state machine under a short-lived lock.
    let outcome: AttributionOutcome = {
        let mut guard = continuity.lock().await;
        let (outcome, new_state) =
            continuity::next_attribution(&*guard, end_ms, evidence, config);
        *guard = new_state;
        outcome
    };

    let persisted_score: Option<f64> = if outcome.carried_over {
        None
    } else {
        match_similarity
    };

    // Candidate-clip retention keys off the FINAL outcome, not the raw
    // matcher result. Carry-over turns an unknown into an attributed
    // segment, so we should not retain its audio as a Phase-A candidate.
    let audio_ref: Option<String> = if outcome.speaker_id.is_none()
        && audio_quality >= VOICEPRINT_CANDIDATE_QUALITY
    {
        let candidate_id = Uuid::new_v4().to_string();
        match crate::engine::clip_storage::write_clip(clips_dir, &candidate_id, audio) {
            Ok(name) => Some(name),
            Err(err) => {
                warn!(?err, ?clips_dir, "failed to retain voiceprint-candidate clip");
                None
            }
        }
    } else {
        None
    };

    crate::repository::segment::insert_segment(
        pool,
        session_id,
        start_ms,
        end_ms,
        outcome.speaker_id,
        persisted_score,
        embedding,
        audio_ref.as_deref(),
    )
    .await?;

    aggregator.publish_speaker_resolved(
        crate::engine::transcript_aggregator::SpeakerResolvedEvent {
            segment_id: segment_id.to_string(),
            start_ms,
            end_ms,
            speaker_id: outcome.speaker_id.map(|u| u.to_string()),
            confidence: outcome.confidence.map(|c| c.as_str()),
            carried_over: outcome.carried_over,
        },
    );

    Ok(outcome.speaker_id.map(|u| u.to_string()))
}
```

- [ ] **Step 3: Refactor `handle_segment_embedding` to use `finalize_segment` on the identify + too-short paths, keep the raw-insert + publish path for no-model/error/live-enrollment**

Replace the body of `handle_segment_embedding` (entire function body) in `backend/actio-core/src/engine/inference_pipeline.rs` with:

```rust
    // Raw-publish helper for paths that must not touch continuity.
    let publish_raw = |speaker_id: Option<String>,
                       confidence: Option<&'static str>| {
        aggregator.publish_speaker_resolved(
            crate::engine::transcript_aggregator::SpeakerResolvedEvent {
                segment_id: segment_id.to_string(),
                start_ms,
                end_ms,
                speaker_id,
                confidence,
                carried_over: false,
            },
        );
    };

    let Some(model_path) = embedding_model else {
        info!(start_ms, end_ms, "segment hook: no embedding model — marking UNKNOWN");
        crate::repository::segment::insert_segment(
            pool, session_id, start_ms, end_ms, None, None, None, None,
        )
        .await?;
        publish_raw(None, None);
        return Ok(None);
    };

    let emb = match crate::engine::diarization::extract_embedding(&model_path, &audio).await {
        Ok(e) => e,
        Err(err) => {
            warn!(?err, "speaker embedding failed; segment marked UNKNOWN");
            crate::repository::segment::insert_segment(
                pool, session_id, start_ms, end_ms, None, None, None, None,
            )
            .await?;
            publish_raw(None, None);
            return Ok(None);
        }
    };

    let duration_ms = (end_ms - start_ms) as f64;
    let quality = crate::engine::audio_quality::score(&audio);

    // Live-enrollment short-circuit: synthetic Confirmed publish, no
    // continuity touch.
    if let Some(enrolled_speaker) = crate::engine::live_enrollment::consume_segment(
        live_enrollment,
        duration_ms,
        quality,
        pool,
        &emb.values,
    )
    .await?
    {
        let speaker_uuid = Uuid::parse_str(&enrolled_speaker).ok();
        crate::repository::segment::insert_segment(
            pool,
            session_id,
            start_ms,
            end_ms,
            speaker_uuid,
            None,
            Some(&emb.values),
            None,
        )
        .await?;
        publish_raw(
            Some(enrolled_speaker.clone()),
            Some(crate::domain::speaker_matcher::MatchConfidence::Confirmed.as_str()),
        );
        return Ok(Some(enrolled_speaker));
    }

    let config = ContinuityConfig {
        window_ms: speaker_id_config.continuity_window_ms,
    };

    // Duration gate: very short VAD segments give noisy embeddings. Skip
    // the identifier entirely, but still pass Unknown through continuity
    // so a carry-over can rescue the attribution.
    if duration_ms < speaker_id_config.min_duration_ms as f64 {
        info!(
            duration_ms,
            min = speaker_id_config.min_duration_ms,
            "segment hook: skipping speaker-id — too short"
        );
        return finalize_segment(
            pool,
            session_id,
            start_ms,
            end_ms,
            Some(&emb.values),
            &audio,
            quality,
            clips_dir,
            MatchEvidence::Unknown,
            None,
            continuity,
            config,
            aggregator,
            segment_id,
        )
        .await;
    }

    info!(
        dim = emb.values.len(),
        start_ms,
        end_ms,
        "segment hook: identifying speaker"
    );
    let thresholds = crate::domain::speaker_matcher::IdentifyThresholds {
        confirm: speaker_id_config.confirm_threshold as f64,
        tentative: speaker_id_config.tentative_threshold as f64,
    };
    let result = crate::domain::speaker_matcher::identify_speaker_with_thresholds(
        pool, &emb.values, tenant_id, thresholds,
    )
    .await
    .unwrap_or(crate::domain::speaker_matcher::SpeakerMatchResult {
        speaker_id: None,
        similarity_score: 0.0,
        z_norm_score: 0.0,
        accepted: false,
        confidence: None,
    });

    let evidence = match (
        result.confidence,
        result
            .speaker_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok()),
    ) {
        (
            Some(crate::domain::speaker_matcher::MatchConfidence::Confirmed),
            Some(id),
        ) => MatchEvidence::Confirmed { speaker_id: id },
        (
            Some(crate::domain::speaker_matcher::MatchConfidence::Tentative),
            Some(id),
        ) => MatchEvidence::Tentative { speaker_id: id },
        _ => MatchEvidence::Unknown,
    };
    let match_similarity = match evidence {
        MatchEvidence::Unknown => None,
        _ => Some(result.similarity_score),
    };

    finalize_segment(
        pool,
        session_id,
        start_ms,
        end_ms,
        Some(&emb.values),
        &audio,
        quality,
        clips_dir,
        evidence,
        match_similarity,
        continuity,
        config,
        aggregator,
        segment_id,
    )
    .await
}
```

- [ ] **Step 4: Update `handle_segment_embedding`'s signature to take the continuity Arc**

Still in `backend/actio-core/src/engine/inference_pipeline.rs`, change the function signature (the `async fn handle_segment_embedding(` line and its argument list) to:

```rust
#[allow(clippy::too_many_arguments)]
async fn handle_segment_embedding(
    pool: &sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    session_id: Uuid,
    tenant_id: Uuid,
    segment_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    audio: Vec<f32>,
    clips_dir: &Path,
    live_enrollment: &LiveEnrollment,
    aggregator: &Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: &Arc<Mutex<ContinuityState>>,
) -> anyhow::Result<Option<String>> {
```

- [ ] **Step 5: Replace `spawn_segment_hook` with `run_segment_hook`**

Still in `backend/actio-core/src/engine/inference_pipeline.rs`, remove `fn spawn_segment_hook(...)` entirely and replace with:

```rust
#[allow(clippy::too_many_arguments)]
async fn run_segment_hook(
    seg: SpeechSegment,
    session_id: Uuid,
    tenant_id: Uuid,
    pool: sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    clips_dir: PathBuf,
    live_enrollment: LiveEnrollment,
    aggregator: Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: Arc<Mutex<ContinuityState>>,
) {
    let start_ms = (seg.start_sample as i64 * 1000) / 16000;
    let end_ms = (seg.end_sample as i64 * 1000) / 16000;
    let segment_id = seg.segment_id;
    let audio = seg.audio;

    if let Err(e) = handle_segment_embedding(
        &pool,
        embedding_model,
        session_id,
        tenant_id,
        segment_id,
        start_ms,
        end_ms,
        audio,
        &clips_dir,
        &live_enrollment,
        &aggregator,
        speaker_id_config,
        &continuity,
    )
    .await
    {
        warn!(%session_id, error = %e, "segment speaker-id hook failed");
    }
}
```

- [ ] **Step 6: Switch `split_segments_for_speaker_id` to inline forward-first**

Still in `backend/actio-core/src/engine/inference_pipeline.rs`, replace `fn split_segments_for_speaker_id(...)` with:

```rust
#[allow(clippy::too_many_arguments)]
fn split_segments_for_speaker_id(
    mut upstream: mpsc::Receiver<SpeechSegment>,
    session_id: Uuid,
    tenant_id: Uuid,
    pool: sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    clips_dir: PathBuf,
    live_enrollment: LiveEnrollment,
    aggregator: Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: Arc<Mutex<ContinuityState>>,
) -> mpsc::Receiver<SpeechSegment> {
    let (tx, rx) = mpsc::channel::<SpeechSegment>(32);
    tokio::spawn(async move {
        while let Some(seg) = upstream.recv().await {
            // 1. Forward to downstream offline ASR FIRST so transcription
            //    is not delayed by the embedding + identify hop.
            if tx.send(seg.clone()).await.is_err() {
                break; // downstream ASR consumer went away
            }
            // 2. Run the speaker-id hook inline. The next recv() does not
            //    fire until this returns, so the state machine sees
            //    segments strictly in VAD emission order.
            run_segment_hook(
                seg,
                session_id,
                tenant_id,
                pool.clone(),
                embedding_model.clone(),
                clips_dir.clone(),
                live_enrollment.clone(),
                aggregator.clone(),
                speaker_id_config,
                continuity.clone(),
            )
            .await;
        }
    });
    rx
}
```

- [ ] **Step 7: Switch `spawn_speaker_id_only` to inline (no forward to swap)**

Replace `fn spawn_speaker_id_only(...)` with:

```rust
#[allow(clippy::too_many_arguments)]
fn spawn_speaker_id_only(
    mut upstream: mpsc::Receiver<SpeechSegment>,
    session_id: Uuid,
    tenant_id: Uuid,
    pool: sqlx::SqlitePool,
    embedding_model: Option<PathBuf>,
    clips_dir: PathBuf,
    live_enrollment: LiveEnrollment,
    aggregator: Arc<TranscriptAggregator>,
    speaker_id_config: SpeakerIdConfig,
    continuity: Arc<Mutex<ContinuityState>>,
) {
    tokio::spawn(async move {
        while let Some(seg) = upstream.recv().await {
            run_segment_hook(
                seg,
                session_id,
                tenant_id,
                pool.clone(),
                embedding_model.clone(),
                clips_dir.clone(),
                live_enrollment.clone(),
                aggregator.clone(),
                speaker_id_config,
                continuity.clone(),
            )
            .await;
        }
    });
}
```

- [ ] **Step 8: Create the session-scoped continuity Arc in `start_session` and thread it through**

Still in `backend/actio-core/src/engine/inference_pipeline.rs`, modify `start_session`. Near the top of the function (just after `let chosen = asr_model.unwrap_or("auto");` log line, and before the `start_vad_pipeline` closure), add:

```rust
        // Session-scoped state: a fresh Arc each call. Dropping this
        // handle on the next start_session isolates stale in-flight tasks
        // from the new session's state automatically.
        let continuity: Arc<Mutex<ContinuityState>> =
            Arc::new(Mutex::new(ContinuityState::default()));
```

Then in the `start_vad_pipeline` closure (`Ok(split_segments_for_speaker_id(...))`), add `continuity.clone(),` as the last argument, aligned with the new parameter added in Step 6.

And in the `start_parallel_speaker_vad` closure (`spawn_speaker_id_only(...)`), add `continuity.clone(),` as the last argument, aligned with the new parameter added in Step 7.

- [ ] **Step 9: Verify everything compiles**

Run: `cd /d/Dev/Actio/backend && cargo check -p actio-core`
Expected: `Finished ... dev [unoptimized + debuginfo]` with no errors. Warnings about unused imports are acceptable and will be cleaned up by Step 10's test run.

- [ ] **Step 10: Run the full backend test suite**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core`
Expected: 82 passed (same as end of Task 4). `handle_segment_embedding` still emits the same events for Confirmed/Tentative/Unknown matches today — carry-over only fires when state has been seeded by a prior Confirmed, which tests don't yet exercise. Task 9 adds that exercise.

- [ ] **Step 11: Commit**

```bash
git add backend/actio-core/src/engine/inference_pipeline.rs
git commit -m "feat(pipeline): extract finalize_segment + inline forward-first continuity"
```

---

### Task 9: Integration test for `finalize_segment`

**Files:**
- Modify: `backend/actio-core/src/engine/inference_pipeline.rs` (add a `#[cfg(test)] mod tests`)

- [ ] **Step 1: Add a tests module that exercises carry-over end-to-end**

At the very bottom of `backend/actio-core/src/engine/inference_pipeline.rs`, append:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::continuity::{ContinuityConfig, ContinuityState, MatchEvidence};
    use crate::repository::db::run_migrations;
    use crate::repository::speaker::create_speaker;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::path::PathBuf;

    async fn fresh_pool() -> sqlx::SqlitePool {
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

    #[tokio::test]
    async fn finalize_segment_carries_over_after_confirmed() {
        let pool = fresh_pool().await;
        let speaker = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let speaker_uuid = Uuid::parse_str(&speaker.id).unwrap();

        let aggregator = Arc::new(TranscriptAggregator::new(pool.clone()));
        let mut speaker_rx = aggregator.subscribe_speaker();

        let continuity: Arc<Mutex<ContinuityState>> =
            Arc::new(Mutex::new(ContinuityState::default()));
        let config = ContinuityConfig { window_ms: 15_000 };
        let clips_dir = PathBuf::from(std::env::temp_dir()).join("actio-test-clips");
        std::fs::create_dir_all(&clips_dir).unwrap();
        let session_id = Uuid::new_v4();
        let audio: Vec<f32> = vec![0.0; 16_000]; // 1s of silence — won't trigger clip retention
        let embedding: Vec<f32> = vec![0.1_f32; 192];

        // 1. Confirmed match for our speaker at segment ending at 3_000 ms.
        let seg_id_1 = Uuid::new_v4();
        finalize_segment(
            &pool,
            session_id,
            0,
            3_000,
            Some(&embedding),
            &audio,
            0.5, // quality below retention threshold — no clip write
            &clips_dir,
            MatchEvidence::Confirmed { speaker_id: speaker_uuid },
            Some(0.72),
            &continuity,
            config,
            &aggregator,
            seg_id_1,
        )
        .await
        .unwrap();

        // 2. Unknown evidence 5_000 ms later — well within 15_000 window.
        let seg_id_2 = Uuid::new_v4();
        finalize_segment(
            &pool,
            session_id,
            3_000,
            8_000,
            Some(&embedding),
            &audio,
            0.5,
            &clips_dir,
            MatchEvidence::Unknown,
            None,
            &continuity,
            config,
            &aggregator,
            seg_id_2,
        )
        .await
        .unwrap();

        // Assert the persisted rows.
        let rows: Vec<(String, Option<String>, Option<f64>)> = sqlx::query_as(
            "SELECT id, speaker_id, speaker_score FROM audio_segments \
             WHERE session_id = ?1 ORDER BY start_ms",
        )
        .bind(session_id.to_string())
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2, "two segments should be persisted");

        let row1 = &rows[0];
        assert_eq!(
            row1.1.as_deref(),
            Some(speaker.id.as_str()),
            "Confirmed row should carry the matched speaker id"
        );
        assert!(row1.2.is_some(), "Confirmed row should persist the similarity");
        assert!((row1.2.unwrap() - 0.72).abs() < 1e-6);

        let row2 = &rows[1];
        assert_eq!(
            row2.1.as_deref(),
            Some(speaker.id.as_str()),
            "Unknown-within-window row should carry over the previous speaker"
        );
        assert!(
            row2.2.is_none(),
            "carry-over rows must persist speaker_score = NULL"
        );

        // Assert both speaker_resolved events fired with the expected flags.
        let ev1 = speaker_rx
            .try_recv()
            .expect("first event should be buffered");
        assert_eq!(ev1.speaker_id.as_deref(), Some(speaker.id.as_str()));
        assert_eq!(ev1.confidence, Some("confirmed"));
        assert!(!ev1.carried_over);

        let ev2 = speaker_rx
            .try_recv()
            .expect("second event should be buffered");
        assert_eq!(ev2.speaker_id.as_deref(), Some(speaker.id.as_str()));
        assert_eq!(ev2.confidence, Some("tentative"));
        assert!(ev2.carried_over, "second event should be flagged as carried over");

        // Continuity state should still point at Alice and still hold the
        // Confirmed timestamp (carry-over did not self-extend).
        let state = continuity.lock().await;
        assert_eq!(state.speaker_id, Some(speaker_uuid));
        assert_eq!(state.last_confirmed_ms, Some(3_000));
    }
}
```

- [ ] **Step 2: Run the new test**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core inference_pipeline::tests::finalize_segment_carries_over_after_confirmed`
Expected: PASS.

- [ ] **Step 3: Run the full backend suite**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core`
Expected: 83 passed (82 previous + the new integration test).

- [ ] **Step 4: Commit**

```bash
git add backend/actio-core/src/engine/inference_pipeline.rs
git commit -m "test(pipeline): finalize_segment carries over after a Confirmed match"
```

---

### Task 10: Expand `patch_settings` restart comparison

**Files:**
- Modify: `backend/actio-core/src/api/settings.rs`

- [ ] **Step 1: Replace the existing two-field comparison with a six-field tuple**

In `backend/actio-core/src/api/settings.rs`, find the block inside `pub async fn patch_settings` that compares `old_asr` / `new_asr` / `old_embed` / `new_embed`. Replace the snapshot + comparison block with:

```rust
    // Snapshot speaker-ID-affecting fields before the patch. Any change in
    // this tuple warrants a pipeline restart because SpeakerIdConfig is
    // copied by value into spawned tasks — a running pipeline will not see
    // threshold or window edits until it restarts.
    let old = state.settings_manager.get().await;
    let old_speaker_tuple = (
        old.audio.asr_model.clone(),
        old.audio.speaker_embedding_model.clone(),
        old.audio.speaker_confirm_threshold,
        old.audio.speaker_tentative_threshold,
        old.audio.speaker_min_duration_ms,
        old.audio.speaker_continuity_window_ms,
    );

    let llm_changed = patch.llm.is_some();
    let new_settings = state.settings_manager.update(patch).await;

    let new_speaker_tuple = (
        new_settings.audio.asr_model.clone(),
        new_settings.audio.speaker_embedding_model.clone(),
        new_settings.audio.speaker_confirm_threshold,
        new_settings.audio.speaker_tentative_threshold,
        new_settings.audio.speaker_min_duration_ms,
        new_settings.audio.speaker_continuity_window_ms,
    );

    if old_speaker_tuple != new_speaker_tuple {
        tracing::info!(
            ?old_speaker_tuple,
            ?new_speaker_tuple,
            "Speaker-ID settings changed — signalling pipeline restart"
        );
        state.pipeline_restart.notify_one();
    }
```

Leave the rest of `patch_settings` (the `llm_changed` block and the `Json(new_settings)` return) intact.

- [ ] **Step 2: Verify compilation and existing tests**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core`
Expected: 83 passed (no new tests; behavior change is observable only at runtime).

- [ ] **Step 3: Commit**

```bash
git add backend/actio-core/src/api/settings.rs
git commit -m "fix(settings): expand pipeline-restart comparison to all speaker-ID fields"
```

---

### Task 11: Frontend Continuity window slider

**Files:**
- Modify: `frontend/src/components/settings/AudioSettings.tsx`

- [ ] **Step 1: Extend the `AudioSettingsShape` interface**

In `frontend/src/components/settings/AudioSettings.tsx`, find the `interface AudioSettingsShape { ... }` block and add:

```tsx
  speaker_continuity_window_ms?: number;
```

- [ ] **Step 2: Add state for the new slider**

In `export function AudioSettings()`, alongside the existing `useState` declarations for `confirmT`, `tentativeT`, `minMs`, add:

```tsx
  const [continuityMs, setContinuityMs] = useState(15000);
```

- [ ] **Step 3: Load the value from settings in the effect**

In the `useEffect(() => { Promise.all([fetchDevices(), fetchSettings()]) ...` block, inside the `.then` callback, add after the `setMinMs(...)` clause:

```tsx
        if (typeof settings.audio?.speaker_continuity_window_ms === 'number') {
          setContinuityMs(settings.audio.speaker_continuity_window_ms);
        }
```

- [ ] **Step 4: Add the commit helper**

Below the existing `commitMinMs` function, add:

```tsx
  const commitContinuity = async (v: number) => {
    setError(null);
    try {
      await patchAudio({ speaker_continuity_window_ms: v });
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    }
  };
```

- [ ] **Step 5: Render the slider below the min-duration slider**

Inside the JSX, immediately after the `<label>` block that wraps the "Min speech duration" slider, insert:

```tsx
      <label className="settings-row">
        <span className="settings-row__label">
          Continuity window{' '}
          <code>{continuityMs === 0 ? 'Off' : `${Math.round(continuityMs / 1000)} s`}</code>
        </span>
        <input
          type="range"
          min="0"
          max="60000"
          step="1000"
          value={continuityMs}
          onChange={(e) => setContinuityMs(parseInt(e.target.value, 10))}
          onMouseUp={() => void commitContinuity(continuityMs)}
          onTouchEnd={() => void commitContinuity(continuityMs)}
          onBlur={() => void commitContinuity(continuityMs)}
        />
      </label>
```

- [ ] **Step 6: Type-check the frontend**

Run: `cd /d/Dev/Actio/frontend && pnpm exec tsc --noEmit`
Expected: no output (clean).

- [ ] **Step 7: Run the frontend tests**

Run: `cd /d/Dev/Actio/frontend && pnpm test -- --run`
Expected: 48 passed (no behavior change for the test suite; AudioSettings is not unit-tested).

- [ ] **Step 8: Commit**

```bash
git add frontend/src/components/settings/AudioSettings.tsx
git commit -m "feat(settings-ui): add Continuity window slider to AudioSettings"
```

---

### Task 12: Final full-suite verification

**Files:**
- None (verification only)

- [ ] **Step 1: Run the backend test suite**

Run: `cd /d/Dev/Actio/backend && cargo test -p actio-core`
Expected: 83 passed, 0 failed.

- [ ] **Step 2: Run the frontend type check**

Run: `cd /d/Dev/Actio/frontend && pnpm exec tsc --noEmit`
Expected: clean.

- [ ] **Step 3: Run the frontend test suite**

Run: `cd /d/Dev/Actio/frontend && pnpm test -- --run`
Expected: 48 passed.

- [ ] **Step 4: Run clippy to surface obvious lints**

Run: `cd /d/Dev/Actio/backend && cargo clippy -p actio-core --no-deps -- -D warnings 2>&1 | tail -20`
Expected: no errors. If warnings surface in new code (e.g. unused imports, `#[allow(clippy::too_many_arguments)]` missing on a new helper), fix them in place and commit with message `chore(continuity): clippy cleanup`.

- [ ] **Step 5: Summarize shipped commits**

Run: `cd /d/Dev/Actio && git log --oneline feat/speaker-diarization ^main | head -20`
Expected: a clean sequence from Task 1 through Task 12 plus any clippy-cleanup follow-ups. Verify each message reflects its task. If any task was split into more commits than the plan expected, that's fine; if any task was squashed, call it out when reporting back to the user.

---

## Spec coverage map (self-review)

- Problem / Goal / Non-goals / Scope — no code; no task needed.
- Architecture → Module layout → **Task 1**.
- Architecture → Data model (`ContinuityState`, `ContinuityConfig`, `MatchEvidence`, `AttributionOutcome`) — **Task 1**.
- Architecture → State location & lifecycle (session-scoped Arc, no touch in `stop()`) — **Task 8** Step 8.
- Architecture → Ordering (inline, forward-first, segment-time clock) — **Task 8** Steps 5–8.
- Architecture → Config plumbing (`SpeakerIdConfig.continuity_window_ms`, tuple restart) — **Task 6** and **Task 10**.
- Decision table (Confirmed / Tentative / Unknown rows) — **Tasks 2, 3, 4**.
- Invariants 1–4 — covered by the test set in **Tasks 2, 3, 4**.
- Accepted tradeoff: rapid-switch smearing — covered by Task 3 Step 5's test and documented in the spec.
- Integration point — `finalize_segment` in **Task 8** Step 2; early-return table enforced in **Task 8** Step 3.
- Updated `SpeakerResolvedEvent` + `is_false` helper — **Task 7**.
- Persistence semantics (NULL score for carry-over, outcome-keyed clip retention) — **Task 8** Step 2 inside `finalize_segment`.
- Frontend (no behavior change v1, permissive parsing) — no task needed; the new field is just ignored by the existing store.
- Testing → Unit tests 1–9 — **Tasks 2, 3, 4**.
- Testing → Integration test — **Task 9**.
- Settings UI — **Task 11**.
- Rollout — no code; behaviour inherited from Task 10's restart comparison and Task 5's default value.

No spec requirement is left without a task.
