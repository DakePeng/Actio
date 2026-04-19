# Speaker Continuity (Temporal Attribution) — Design Spec

**Date:** 2026-04-18
**Status:** Revised after codex review (11 findings addressed)
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

- `ContinuityState` — session-scoped state snapshot.
- `ContinuityConfig` — tunable knobs (window duration).
- `MatchEvidence` — the state-machine input, a closed enum that rules out invalid `speaker_id`/`confidence` combinations at the type level.
- `AttributionOutcome` — returned from the decision function; used by the caller for publish + persist.
- `next_attribution(state, segment_end_ms, evidence, config) -> (AttributionOutcome, ContinuityState)` — pure, sync, fully unit-testable. Returns the **new state** alongside the outcome; there is no separate `apply` method.

### Data model

```rust
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
    pub window_ms: u32,   // 0 disables carry-over entirely
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
    pub confidence: Option<MatchConfidence>,  // reuse existing enum
    pub carried_over: bool,
}
```

No database changes. Persisted segment rows continue to carry the final attributed `speaker_id`; carried-over attributions are indistinguishable from fresh matches at the DB layer (the `speaker_score` column is nulled for carry-overs, see Persistence below). The WS event gains a `carried_over` flag so the client *can* render them differently in a later iteration.

### State location and lifecycle — session-scoped

State is **not** on `AppState`. Instead, each call to `InferencePipeline::start_session` creates a fresh `Arc<tokio::sync::Mutex<ContinuityState>>` and captures it into the VAD consumer closures. When `stop()` is called, the pipeline drops its handle. Any in-flight tasks that somehow still hold a clone of the old Arc see and mutate their own isolated state — which goes nowhere because the new session's state lives in a *different* Arc.

```rust
pub fn start_session(&mut self, ... ) -> anyhow::Result<()> {
    let continuity: Arc<Mutex<ContinuityState>> =
        Arc::new(Mutex::new(ContinuityState::default()));
    // ... captured into split_segments_for_speaker_id / start_parallel_speaker_vad
    // alongside the existing live_enrollment Arc.
}
```

`InferencePipeline::stop()` stays synchronous (the existing signature). It does not need to lock or clear continuity state because the Arc it held is simply dropped on the next `start_session`. No async-in-sync hazard.

### Ordering — inline serialized processing (forward-first)

The current pipeline spawns per-segment speaker-id work via `tokio::spawn` inside `spawn_segment_hook`, which makes completion order ≠ speech order. With concurrent hooks mutating shared state, a later segment can seed state before an earlier one finishes. That is non-deterministic and unacceptable for a state machine.

**This plan processes speaker-id inline in the VAD consumer loop**, i.e. inside the existing `while let Some(seg) = upstream.recv().await { ... }` bodies of `split_segments_for_speaker_id` and `spawn_speaker_id_only`. The current `spawn_segment_hook` helper is replaced by an `async fn run_segment_hook(...)` awaited directly in the loop.

**Critical ordering rule — forward to downstream ASR before awaiting the speaker hook.** In `split_segments_for_speaker_id`, the consumer forwards the segment to the offline ASR receiver *first*, then awaits the speaker hook. Otherwise every offline ASR transcript is delayed by the embedding + identify latency (~200–400 ms).

```rust
// split_segments_for_speaker_id body — offline ASR path
while let Some(seg) = upstream.recv().await {
    // 1. Forward to downstream ASR immediately (no back-pressure on transcription).
    if tx.send(seg.clone()).await.is_err() { break; }
    // 2. Now run the speaker-id hook serially — the next recv() won't fire
    //    until this returns, so the state machine sees segments in VAD order.
    run_segment_hook(seg, ..., &continuity, ...).await;
}
```

`spawn_speaker_id_only` (streaming-ASR branch, where ASR consumes raw audio in parallel) has no downstream forward; it just awaits `run_segment_hook` inline and moves on.

Segments are therefore processed strictly in VAD emission order, so:
- State reads and writes are naturally serialized by the loop.
- Out-of-order hazards are impossible by construction — no seq counters, no reorder buffer.
- The clock used by the state machine is `segment.end_ms` (session-relative), not `Instant::now()`.
- **Offline ASR transcription latency — per segment unchanged, under sustained burst load slightly delayed.** The *current* segment is forwarded to the ASR mpsc before the speaker hook awaits, so its ASR processing starts immediately. However, the loop cannot call `upstream.recv().await` for the *next* segment until the speaker hook returns, so when VAD produces segments faster than the hook finishes, later segments queue in the VAD crossbeam channel and their ASR forwarding is delayed by up to ~(hook_latency × queue_depth). In practice segments arrive every 1–5 s and the hook takes 200–400 ms, so the queue stays near-empty and added latency is negligible; only a rare very-short burst pattern would reveal any slowdown.

**Back-pressure budget.** Embedding (~200–400 ms on CPU) + identify (~5–20 ms) now runs in the VAD consumer loop rather than a detached task. VAD's internal queue is `crossbeam_channel::bounded::<SpeechSegment>(32)`; at typical speech rates (one VAD segment every 1–5 s), this gives 30–160 s of headroom before the consumer loop falls behind. The downstream-ASR mpsc is also buffered (`mpsc::channel::<SpeechSegment>(32)`), so short-term skew is absorbed even if speaker work momentarily lags.

### Config plumbing

`speaker_continuity_window_ms: u32` is added to `AudioSettings` (default **15000**, accepted range 0–60000, `0` disables). It flows into `SpeakerIdConfig`, already threaded through `start_session`. `SpeakerIdConfig` gains a `continuity_window_ms: u32` field alongside the existing `confirm_threshold`, `tentative_threshold`, `min_duration_ms`.

`patch_settings` is updated to compare **all audio fields that affect speaker identification** (not just `asr_model` / `speaker_embedding_model`) and fire `pipeline_restart.notify_one()` when any change. Concretely, a tuple comparison on `(asr_model, speaker_embedding_model, speaker_confirm_threshold, speaker_tentative_threshold, speaker_min_duration_ms, speaker_continuity_window_ms)` before and after the patch. This fixes a latent bug where the previously-shipped threshold fields did *not* actually trigger restart despite claims to the contrary.

---

## Decision table (next_attribution)

`within_window(state, now)` = `state.last_confirmed_ms.map_or(false, |t| now - t <= config.window_ms as i64)`. When `config.window_ms == 0`, `within_window` is always false, which collapses the table to rows 1, 4, and 6.

| Evidence | Within window? | Outcome | New state |
|---|---|---|---|
| Confirmed { A } | — | (A, Confirmed, `carried_over=false`) | `{ A, now }` |
| Tentative { A } where state.speaker_id == A | yes | (A, Tentative, `carried_over=false`) | `{ A, now }` (refresh) |
| Tentative { B } where state.speaker_id == A, B ≠ A | yes | (A, Tentative, `carried_over=true`) | unchanged |
| Tentative { X } | no | (X, Tentative, `carried_over=false`) | unchanged |
| Unknown | yes | (state.speaker_id, Tentative, `carried_over=true`) | unchanged |
| Unknown | no | (None, None, `carried_over=false`) | unchanged |

Invariants:
1. **Only Confirmed seeds state.** A tentative alone never creates state out of nothing.
2. **Only Confirmed-to-anyone or same-speaker Tentative refreshes the timer.** Different-speaker Tentative is treated as weak evidence and ignored for both attribution and timer purposes when state is live.
3. **Carried-over attributions never update state.** They are UX courtesy, not new evidence; allowing them to self-reinforce would let one old Confirmed propagate indefinitely.
4. **Segment time is the continuity clock.** `next_attribution` takes `segment_end_ms: i64` as its clock parameter, guaranteeing monotonic ordering regardless of processing time.

### Accepted tradeoff: rapid-switch smearing

Row 3 (different-speaker Tentative ignored while state is live) will briefly mis-attribute a first Tentative-only segment from a new speaker back to the previous active speaker, until they produce a Confirmed match. In practice this typically means one segment of wrong attribution at a conversational turn boundary. We accept this explicitly — the cost of not trusting weak evidence to overturn strong recent evidence. If the smear becomes a usability issue, a follow-up can add a "strong switch" threshold that allows a Tentative with `sim >= switch_threshold` to flip state.

---

## Integration point

`handle_segment_embedding` is refactored to funnel every decision path through a common finalization step. The extraction boundary is a new testable helper:

```rust
async fn finalize_segment(
    pool: &SqlitePool,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    embedding: Option<&[f32]>,
    audio: &[f32],
    clips_dir: &Path,
    evidence: MatchEvidence,
    match_similarity: Option<f64>,   // Some when evidence came from identify; None for too-short/errors
    continuity: &Arc<Mutex<ContinuityState>>,
    config: ContinuityConfig,
    aggregator: &Arc<TranscriptAggregator>,
    segment_id: Uuid,
) -> anyhow::Result<Option<String>>
```

`finalize_segment`:
1. Locks `continuity`, runs `next_attribution`, replaces the state with the returned new state, releases the lock.
2. Decides persistence values:
   - `persisted_speaker_id = outcome.speaker_id`
   - `persisted_score = if outcome.carried_over { None } else { match_similarity }`
   - `candidate_retention_eligible = outcome.speaker_id.is_none() && quality_passes`
     (keyed off the **outcome**, not the raw matcher result — carried-over segments are not candidates for enrollment prompts).
3. Writes `audio_segments` row with `persisted_speaker_id`, `persisted_score`, embedding, optional `audio_ref`.
4. Publishes `SpeakerResolvedEvent` with `outcome.speaker_id`, `outcome.confidence.as_str()`, and `outcome.carried_over`.

### Early-return paths — what each does with continuity

Enumerating every path currently inside `handle_segment_embedding`:

| Condition | Evidence handed to `finalize_segment` | Continuity reads? | Continuity writes? | Rationale |
|---|---|---|---|---|
| No embedding model loaded | (skipped) | No | No | Model unavailable ≠ "silence"; don't let it seed or flip state. Row inserted with `speaker_id=None`, event published with `None/None/carried_over=false`. |
| Embedding extraction error | (skipped) | No | No | Same reasoning. |
| Live enrollment consumed the segment | (skipped) | No | No | Live enrollment is synthetic. It does not represent a natural speech turn and should neither seed nor refresh continuity. The event still publishes Confirmed under the enrolled speaker id as today. |
| VAD segment shorter than `min_duration_ms` | `Unknown` | Yes | Yes (per state machine) | Short clips pass through continuity — this is the primary case where carry-over matters. |
| Identify returned confidence = Confirmed | `Confirmed { id }` | Yes | Yes | |
| Identify returned confidence = Tentative | `Tentative { id }` | Yes | Yes | |
| Identify returned confidence = None (below tentative, or matcher DB error) | `Unknown` | Yes | Yes | Includes the case where the matcher's `Result` was `Err` and the pipeline fell back to the `unwrap_or(default)` path. |

The no-embedding-model, embedding-error, and live-enrollment branches call a separate `insert_raw_segment` + publish path that does not touch continuity. The other four call `finalize_segment`.

### Updated `SpeakerResolvedEvent`

```rust
pub struct SpeakerResolvedEvent {
    // existing fields: segment_id, start_ms, end_ms, speaker_id, confidence
    pub carried_over: bool,
}
```

`WsSpeakerResolvedEvent` mirrors it. To keep the wire payload compact when the flag is false, use a tiny helper and point `skip_serializing_if` at it:

```rust
fn is_false(b: &bool) -> bool { !*b }

#[derive(Serialize)]
struct WsSpeakerResolvedEvent {
    // ...
    #[serde(skip_serializing_if = "is_false")]
    carried_over: bool,
}
```

(`std::ops::Not::not` cannot be used directly because its signature does not match serde's expected `fn(&bool) -> bool`.)

---

## Persistence semantics

For every row `finalize_segment` writes:

- `speaker_id` = `outcome.speaker_id` (so carried-over rows are attributed just like confirmed ones; downstream consumers see a unified story).
- `speaker_score` = `Some(similarity_score)` when evidence came from identify and `carried_over == false`; `None` otherwise (carry-over, too-short, matcher error). Previously the score was always forced; we stop inventing `0.0` for paths that have no real similarity.
- `embedding` = always the real embedding when we have one. Carried-over or not, the raw evidence is preserved for future offline refinement.
- `audio_ref` (candidate clip retention) = only set when `outcome.speaker_id.is_none() && quality >= VOICEPRINT_CANDIDATE_QUALITY`. Carry-over converts an unknown into an attributed segment; it should therefore *not* be retained as a Phase-A candidate.

---

## Frontend

No behaviour change in v1. Carried-over segments arrive as existing `speaker_resolved` events with `confidence: "tentative"` and render with the existing `?` badge — which is exactly what we want: "we think this is you, but we inferred it from context." The new `carried_over` field on the wire is permitted by the frontend's permissive JSON parsing but discarded by the store. The field will be surfaced on `TranscriptLine` when the v2 visual polish (distinct "inferred" badge tone) lands.

---

## Testing

### Unit tests (pure `next_attribution`)

All run in `continuity.rs` `#[cfg(test)] mod tests`. No async, no pool, no pipeline. `MatchEvidence` is constructed by hand; `segment_end_ms` is an integer the test supplies directly. Each test asserts `(outcome, new_state)` against the decision table.

1. **Confirmed seeds state.** Empty state, Confirmed{A} at t=0. Outcome = (A, Confirmed, false). New state = `{A, 0}`.
2. **Confirmed flips.** State `{A, 0}`, Confirmed{B} at t=5000. Outcome = (B, Confirmed, false). New state = `{B, 5000}`.
3. **Same-speaker Tentative refreshes timer.** State `{A, 0}`, window 15000. Tentative{A} at t=14000 → outcome (A, Tentative, false), state `{A, 14000}`. Then Unknown at t=20000 (within 15000 of 14000) → outcome (A, Tentative, true), state unchanged.
4. **Different-speaker Tentative ignored when state live.** State `{A, 0}`, Tentative{B} at t=5000 → outcome (A, Tentative, true), state unchanged.
5. **Different-speaker Tentative with no state.** Empty state, Tentative{B} at any t → outcome (B, Tentative, false), state unchanged.
6. **Unknown carries over within window.** State `{A, 0}`, window 15000. Unknown at t=10000 → outcome (A, Tentative, true), state unchanged.
7. **Unknown outside window drops.** State `{A, 0}`, window 15000. Unknown at t=20000 → outcome (None, None, false), state unchanged.
8. **Carry-over does not self-extend.** State `{A, 0}`, window 15000. Unknown at t=10000 (carry), then Unknown at t=25000 → second call returns (None, None, false), confirming the first carry did not refresh the timer.
9. **Window=0 disables carry-over.** State `{A, 0}`, window 0. Unknown at t=1 → outcome (None, None, false), state unchanged. Confirmed and Tentative rows still behave normally.
*(A prior draft included a test claiming `next_attribution` is "monotonic regardless of input ordering". That claim is false — the state machine has no stale-segment guard, and in-order execution is a property of the **caller** (the inline VAD consumer loop), not of the pure function. Removed to avoid encoding a guarantee we don't provide. If an offline refiner ever replays segments out of order, it will need its own ordering discipline or an explicit stale-segment rejection rule added here.)*

### Integration test (pipeline wiring)

One `#[tokio::test]` in `inference_pipeline` tests module, added against the extracted `finalize_segment` helper rather than the full hook:

- Set up an in-memory `SqlitePool`, run migrations, create one speaker.
- Call `finalize_segment` twice:
  1. Evidence = `Confirmed{speaker_id}`, `match_similarity=Some(0.7)`.
  2. Evidence = `Unknown`, `match_similarity=None`.
- Assert row 1 persists with `speaker_id=speaker`, `speaker_score≈0.7`, continuity state now `{speaker, end_ms_1}`.
- Assert row 2 persists with `speaker_id=speaker` (carried over), `speaker_score=NULL`, continuity state unchanged.
- Assert both `SpeakerResolvedEvent`s were published with the expected `carried_over` flags.

This test does not touch the ONNX embedding model. Tests of the real matcher already live in `speaker_matcher.rs`.

---

## Settings UI

One new slider in **Settings → Audio → Speaker Recognition** below the existing three:

- Label: `Continuity window`
- Value display: seconds (e.g. `15 s`); `0` renders as `Off`.
- Range: `0`–`60000` ms, step `1000`.

Same commit-on-release pattern (`onMouseUp` / `onBlur`) as the other sliders, using the existing `patchAudio` helper. The backend restart comparison change (above) makes edits take effect on next pipeline cycle without requiring a full app reload.

---

## Rollout

- No DB migrations.
- No backfill of existing transcripts.
- No persisted schema changes (only the value range for `speaker_score` changes: it's now explicitly `NULL` for carry-over rows, which is already a legal value for that column).
- Takes effect on next pipeline restart. Restart fires when the speaker-ID tuple `(asr_model, speaker_embedding_model, speaker_confirm_threshold, speaker_tentative_threshold, speaker_min_duration_ms, speaker_continuity_window_ms)` changes, or on hibernate/wake cycles. Other `AudioSettings` fields (e.g. `device_name`, `clip_retention_days`) do **not** trigger a restart from this comparison — that's intentional; mic-change handling is out of scope for this spec.

If the behaviour proves wrong or surprising in use, setting `speaker_continuity_window_ms = 0` disables carry-over completely and reverts to pre-continuity behaviour — no code change required.
