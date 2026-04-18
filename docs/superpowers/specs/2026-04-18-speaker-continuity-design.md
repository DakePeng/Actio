# Speaker Continuity (Temporal Attribution) — Design Spec

**Date:** 2026-04-18
**Status:** Approved, ready for writing-plans
**Related:** Builds on the 2026-04-17 speaker-diarization spec and the matcher-tier work shipped 2026-04-18 (`speaker_matcher::MatchConfidence`, `SpeakerIdConfig`).

---

## Problem

A single continuous utterance from one speaker in the same room and microphone gets chopped by VAD into multiple segments. Per-segment speaker-embedding cosine similarity varies noticeably across those segments — some land above the confirm threshold, some in the tentative zone, some below. Result: one real speech turn from one speaker renders as a mix of confidently-attributed, tentative, and unknown bubbles.

The root cause is per-segment embedding instability (short clips, within-utterance phonetic variance, brief noise bursts), not the speaker's voice actually changing across conditions. A multi-circumstance prototype cluster would not fix this; temporal continuity will.

## Goal

When recent evidence shows one speaker has been confidently active, inherit that attribution for adjacent segments whose own match is weak or absent, so a single speech turn renders coherently under one speaker.

## Non-goals

- **Multi-circumstance / adaptive prototype clusters.** Solves condition drift, not per-segment noise. Parked.
- **Audio-level segment merging** (concatenating adjacent VAD segments before embedding). Adds latency and complicates the streaming/offline pipeline split; worth revisiting if continuity alone proves insufficient.
- **Offline re-attribution pass over saved transcripts.** Out of scope; future offline refiner concern.
- **Visual differentiation between matched-tentative and carried-over attributions** (e.g. different badge colour). Listed as a v2 polish.

## Scope of behaviour change

Only the per-segment speaker-identification step changes. VAD, ASR, enrollment, embedding extraction, and DB schema are untouched. No migrations.

---

## Architecture

### Module layout

A new module `backend/actio-core/src/engine/continuity.rs` owns:

- `ContinuityState` — in-memory per-pipeline state.
- `ContinuityConfig` — tunable knobs (window duration).
- `AttributionOutcome` — the value returned from the decision function and used by the caller for publish + persist.
- `next_attribution(...)` — pure, sync, fully unit-testable.
- `ContinuityState::apply(&AttributionOutcome)` — updates state per the state-machine rules.

### Data model

```rust
pub struct ContinuityState {
    pub speaker_id: Option<Uuid>,
    pub last_confirmed_at: Option<Instant>,
}

pub struct ContinuityConfig {
    pub window_ms: u32,   // 0 disables carry-over
}

pub struct AttributionOutcome {
    pub speaker_id: Option<Uuid>,
    pub confidence: Option<MatchConfidence>,  // reuse existing enum
    pub carried_over: bool,
}
```

No database changes. Persisted segment rows continue to carry the final attributed `speaker_id`; carried-over attributions are indistinguishable from fresh matches at the DB layer, which is correct — downstream consumers (offline refiner, LLM summary) shouldn't treat them differently.

### State plumbing

A new field on `AppState`:

```rust
pub continuity: Arc<tokio::sync::Mutex<ContinuityState>>,
```

Initialised to `ContinuityState::default()` alongside the other mutex-guarded pipeline state. Cloned into `spawn_segment_hook` the same way `live_enrollment` is.

Lifecycle:
- `InferencePipeline::stop()` clears state (drops `speaker_id`, `last_confirmed_at`).
- Top of `start_session` clears state before the new pipeline goes live.

Only one session runs at a time (enforced by `pipeline.is_running()`), so a single instance — not a `HashMap` keyed by session — is sufficient.

### Config plumbing

`speaker_continuity_window_ms: u32` is added to `AudioSettings` (default **15000**, accepted range 0–60000, `0` disables). It flows into `SpeakerIdConfig`, already threaded through `start_session`. `SpeakerIdConfig` gains a `continuity_window_ms` field alongside the existing `confirm_threshold`, `tentative_threshold`, `min_duration_ms`.

`patch_settings` includes the new field in its "audio model selection changed" comparison so a window edit triggers `pipeline_restart.notify_one()` the same way threshold edits do.

---

## Decision table (next_attribution)

"Within window" = `now - last_confirmed_at <= window`. When `window_ms == 0`, carry-over is globally disabled and only the first and last rows apply.

| Match result | State active? | Outcome | State update |
|---|---|---|---|
| Confirmed (any speaker) | — | speaker_id from match, Confirmed, `carried_over=false` | `speaker_id = match.speaker_id`, `last_confirmed_at = now` |
| Tentative, same speaker as state | yes | speaker_id from match, Tentative, `carried_over=false` | `last_confirmed_at = now` (refresh only) |
| Tentative, different speaker from state | yes | state.speaker_id, Tentative, `carried_over=true` | no change |
| Tentative, any speaker | no | speaker_id from match, Tentative, `carried_over=false` | no change |
| Unknown / too-short / below tentative | yes | state.speaker_id, Tentative, `carried_over=true` | no change |
| Unknown / too-short / below tentative | no | None, None, `carried_over=false` | no change |

Invariants:
1. **Only Confirmed seeds state.** A tentative alone never creates state out of nothing.
2. **Only Confirmed-to-anyone or same-speaker Tentative refreshes the timer.** Different-speaker Tentative is treated as weak evidence and ignored for both attribution and timer purposes when state is live.
3. **Carried-over attributions never update state.** They are UX courtesy, not new evidence; allowing them to self-reinforce would let one old Confirmed propagate indefinitely.
4. **Time is monotonic and injected.** `next_attribution` takes `now: Instant` as a parameter so tests can drive synthetic timelines.

---

## Integration point

Inside `handle_segment_embedding` (in `inference_pipeline.rs`), after `identify_speaker_with_thresholds` returns and before `publish(...)` / `insert_segment(...)`:

```rust
let match_result = identify_speaker_with_thresholds(...).await.unwrap_or_default();

let mut state = continuity_state.lock().await;
let config = ContinuityConfig { window_ms: speaker_id_config.continuity_window_ms };
let outcome = continuity::next_attribution(&*state, Instant::now(), &match_result, config);
state.apply(&outcome);
drop(state);

// Persist + publish use outcome, not match_result
insert_segment(..., outcome.speaker_id, Some(match_result.similarity_score), ...).await?;
publish(outcome.speaker_id.map(|u| u.to_string()), outcome.confidence.map(|c| c.as_str()));
```

The `SpeakerResolvedEvent` gets one new field:

```rust
pub struct SpeakerResolvedEvent {
    // ... existing fields ...
    pub carried_over: bool,
}
```

`WsSpeakerResolvedEvent` mirrors it. Frontend ignores this field for v1 but it's available for the optional v2 styling polish.

---

## Frontend

No behaviour change required in v1. Carried-over segments arrive as existing `speaker_resolved` events with `confidence: "tentative"` and render with the existing `?` badge — which is exactly what we want: "we think this is you, but we inferred it from context." The `carried_over` field on the WS message is accepted by the store's deserializer (so it doesn't error on the new field) but is discarded. It will be surfaced in `TranscriptLine` when the v2 visual polish lands.

---

## Testing

### Unit tests (in `continuity.rs`)

Pure-function tests against `next_attribution`. No async, no DB, no pool.

1. **Confirmed seeds state.** State empty → Confirmed match to A → outcome `{A, Confirmed, carried_over=false}`, state `{A, now}`.
2. **Confirmed flips to another speaker.** State `{A, t0}` → Confirmed match to B at t0+5s → outcome B/Confirmed, state `{B, t0+5}`.
3. **Same-speaker Tentative refreshes.** State `{A, t0}`, window 15s → same-speaker Tentative at t0+14s → outcome A/Tentative/carried=false, state `{A, t0+14}`. Then Unknown at t0+20s (within 15s of the refresh) → outcome A/Tentative/carried=true (carry-over still works).
4. **Different-speaker Tentative ignored when state live.** State `{A, t0}` → Tentative to B at t0+5s → outcome A/Tentative/carried=true, state unchanged.
5. **Different-speaker Tentative with no state.** State empty → Tentative to B → outcome B/Tentative/carried=false, state unchanged.
6. **Unknown carries over within window.** State `{A, t0}`, window 15s → Unknown at t0+10s → outcome A/Tentative/carried=true, state unchanged.
7. **Unknown outside window drops.** State `{A, t0}`, window 15s → Unknown at t0+20s → outcome None/None/carried=false, state unchanged.
8. **Carry-over doesn't self-extend.** State `{A, t0}`, window 15s → Unknown at t0+10s (carry), then Unknown at t0+25s → second call returns None/None (the first carry-over did not reset the timer).
9. **Window=0 disables carry-over.** State `{A, t0}`, window 0 → Unknown at t0+1ms → outcome None/None.
10. **Too-short behaves like Unknown.** State `{A, t0}`, window 15s → too-short (confidence None) within window → outcome A/Tentative/carried=true.

### Integration test

One `#[tokio::test]` in `inference_pipeline` tests module: set up an in-memory pool, enrol one speaker with a known embedding, drive two fake segments through `handle_segment_embedding` — the first with Confirmed-quality audio, the second with below-threshold audio. Assert the persisted segment row for the second segment carries the first's `speaker_id`.

---

## Settings UI

One new slider in **Settings → Audio → Speaker Recognition** below the existing three:

- Label: `Continuity window`
- Value display: seconds (e.g. `15 s`)
- Range: 0–60000 ms, step 1000
- `0` labelled explicitly as "Off" in the track marker

Same commit-on-release pattern (`onMouseUp` / `onBlur`) as the other sliders, using the existing `patchAudio` helper.

---

## Rollout

- No DB migrations.
- No backfill of existing transcripts.
- No persisted field changes.
- Takes effect on next pipeline restart, which happens automatically on any audio-settings patch and on hibernate/wake cycles.

If the behaviour proves wrong or surprising in use, the user can set `speaker_continuity_window_ms = 0` to disable carry-over completely and get back to the pre-continuity behaviour.
