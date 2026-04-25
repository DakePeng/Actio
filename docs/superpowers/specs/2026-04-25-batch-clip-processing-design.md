# Batch Clip Processing — Design

**Date:** 2026-04-25
**Status:** Draft (spec)
**Supersedes (in part):** `2026-04-25-always-on-listening-design.md` — the always-on inference pipeline. The live transcript path from `2026-04-25-live-diarized-transcript-translation-design.md` is preserved but moves into a dedicated streaming service.

## Motivation

The current always-listening pipeline keeps the ASR model hot 24/7, runs ASR streaming-style on each VAD segment as it arrives, and matches speaker embeddings one segment at a time against the voiceprint table. Three real costs:

1. **Quality.** Streaming ASR sees only one segment of context; per-segment speaker matching uses a single 1.5–3 s embedding (noisy). Both could be much better with the full 5-minute clip in hand.
2. **RAM.** The ASR model and the speaker embedder stay loaded even when the user is doing nothing voice-related.
3. **Architecture.** Live UX, persisted archive, and action-item extraction all share one streaming pipeline. Improving any one of them entangles the others.

This redesign splits the work along its natural seam: a thin always-on capture daemon, a deferred batch processor that owns the persisted archive, and a separate on-demand live streaming service for dictation/translation.

## Goals

- ASR is loaded only when (a) a clip is being processed, or (b) dictation/translation is active. Otherwise it is unloaded.
- The persisted transcripts in the archive come from the higher-quality batch pass over the full 5-min clip.
- Speaker attribution uses per-clip global clustering (AHC over all segment embeddings) rather than per-segment online matching, with cluster centroids matched against the voiceprint table.
- Anonymous clusters become auto-provisional voiceprints so subsequent clips link the same person automatically — no enrollment required for cross-clip coherence.
- Live transcripts and translations remain available, but only when the user is actively dictating or translating; they are ephemeral (not written to the DB).
- The user can disable the background archive entirely (privacy mode) without losing dictation/translation.

## Non-goals

- No new ASR/diarization model classes. The existing sherpa-onnx catalog (Zipformer, Whisper, SenseVoice, FunASR, Moonshine, plus the existing speaker-embedding models) is unchanged.
- No retroactive re-attribution of historical transcripts when a new speaker is enrolled. Future work; out of scope here.
- No continuous PCM dump of the user's day. The archive stores per-VAD-segment WAVs only — silence is not recorded.
- No replacement for the live enrollment flow (`live_enrollment.rs`); it keeps its own short-lived dedicated session.

## Locked decisions (from brainstorming)

| Area | Decision |
|---|---|
| Live vs batch persistence | Batch is the source of truth. Live ASR is ephemeral preview — broadcast on `/ws`, never written to the DB. |
| Capture trigger | Always-on (cpal + Silero VAD) whenever the user is unmuted. Mute is the only off-switch. |
| Audio storage | Per-VAD-segment WAVs grouped by clip (no continuous PCM dump). |
| Window boundaries | ~5 min target; force-close on the first ≥1.5 s VAD silence after the 5-min mark; hard cap 6 min. Non-overlapping. |
| Diarization | Per-clip AHC global clustering → centroid match against `voiceprints` → unmatched clusters written as auto-provisional voiceprints. |
| ASR models | Two settings: `live_asr_model` (streaming-friendly) and `archive_asr_model` (batch-friendly), each user-pickable. |
| ASR lifecycle | Cold load per clip; live model loads only while dictation/translation is active. |
| Audio retention | 14 days, then sweep. |
| Voiceprint GC | Auto-GC provisional rows after 30 days unmatched, plus a "Candidate Speakers" panel for promote/dismiss. |
| `always_listening` | Repurposed: gates the background archive. `false` = privacy mode (capture only spins up while the user is actively dictating/translating, and no clips are archived). |
| Legacy infra | Per-failed-segment voiceprint candidate retention retires; folded into per-window storage. |

## Architecture

Three subsystems replace today's single always-on inference pipeline:

```
                   ┌──────────────────────────────────────────────┐
                   │  CaptureDaemon (always on when unmuted)      │
                   │  cpal → Silero VAD → per-segment WAV writer  │
                   └─────────┬────────────────────────────────────┘
                             │ writes segment WAVs + metadata to disk
                             ▼
        ┌────────────────────┴────────────────────┐
        │       ClipBoundaryWatcher               │
        │  closes a clip on first ≥1.5 s VAD gap  │
        │  past the 5-min mark (hard cap 6 min)   │
        │  → enqueues clip onto BatchProcessor    │
        └────────────────────┬────────────────────┘
                             ▼
        ┌────────────────────┴────────────────────┐
        │        BatchProcessor (one at a time)   │
        │  load archive ASR  → transcribe segments│
        │  embed segments    → AHC cluster        │
        │  match centroids   → speakers + provis. │
        │  write transcripts/segments/clip rows   │
        │  unload ASR        → fire post-clip hook│
        └────────────────────┬────────────────────┘
                             ▼
                  existing window_extractor
                  (now: post-clip action items)

   ┌─────────────────────────────────────┐
   │ LiveStreamingService (on-demand)    │   loads live ASR model, runs
   │  spun up by dictation OR translation│   continuity state machine,
   │  taps the same capture stream       │   broadcasts to /ws,
   │  ephemeral — writes nothing to DB   │   feeds translator. Unloads
   └─────────────────────────────────────┘   when both modes are off.
```

The capture daemon and batch processor only run when `always_listening = true`. The live streaming service runs independently of `always_listening` so the user can dictate/translate even with archiving disabled.

## Components

### New

- **`engine/capture_daemon.rs`** — owns the long-lived cpal stream and Silero VAD. Output is tee'd: (a) per-segment WAV writer for the archive, (b) broadcast channel for any subscribed `LiveStreamingService`. Mute toggles the cpal stream entirely.
- **`engine/clip_boundary.rs`** — watches VAD events, closes a clip when conditions hit (target reached + ≥1.5 s gap, or hard cap), writes a `manifest.json` next to the segment WAVs, inserts an `audio_clips` row with `status='pending'`.
- **`engine/batch_processor.rs`** — single-worker async task that drains the `audio_clips` queue. Loads archive ASR (cold), runs ASR + embedding + AHC clustering + voiceprint matching + auto-provisional insertion, writes transcripts/segments, marks clip `processed`, unloads ASR. Reuses the existing `EMBEDDING_WORKERS` LRU.
- **`engine/cluster.rs`** — pure agglomerative hierarchical clustering over a slice of `(segment_id, embedding)` pairs with a tunable cosine threshold (default ~0.4). Returns `Vec<(segment_id, cluster_idx)>`. Pure function, trivially testable.
- **`engine/live_streaming.rs`** — replaces today's `InferencePipeline` for the on-demand live path. Subscribes to the capture daemon, runs per-segment ASR + the existing continuity state machine, broadcasts `transcript` + `speaker_resolved` frames on `/ws`. **No DB writes.**
- **`repository/audio_clip.rs`** — CRUD: `claim_next_pending`, `mark_processed`, `mark_failed`, `requeue_stale_running` (mirrors `extraction_window` repo).

### Repurposed

- **`engine/window_extractor.rs`** — no longer schedules its own windows. Triggered by the BatchProcessor's post-clip hook with the just-written transcripts for that clip. The LLM call, confidence gating (high → open / medium → pending / else dropped), and `source_window_id` reminder writes stay identical. The atomic `claim_next_pending` survives, but its inputs come from `audio_clips` instead of synthetic time-windows. The `SAFETY_MARGIN_MS` of 30 s retires (a clip's transcripts land atomically; there is no "live tail" risk).
- **`engine/inference_pipeline.rs`** — the streaming path internals migrate into `live_streaming.rs`. The `start_session` / `stop` API changes meaning: it now starts/stops the live streaming service, not the persisted-archive pipeline.
- **`pipeline_supervisor` in `lib.rs`** — supervises three things instead of one:
  - **CaptureDaemon** — running iff (`always_listening = true`) or (`live streaming requested`); paused when muted.
  - **BatchProcessor** — running iff `always_listening = true`.
  - **LiveStreamingService** — running iff dictation or translation is active.
  The `IDLE_GRACE_PERIOD` hibernation path retires; nothing in the new model uses subscriber-count-based hibernation.

### Retired

- `clip_storage.rs::write_clip` for per-failed-segment voiceprint-candidate retention. The disk-sweep helper (`start_cleanup_task`) is generalized to sweep the new per-clip audio dir at 14 days.
- The `IDLE_GRACE_PERIOD` hibernation path in `pipeline_supervisor`.
- `continuity.rs` for the persisted/archive path. It stays alive for the live streaming path.

## Data flow & storage

### On disk

```
<clips_dir>/<session_id>/<clip_id>/
    seg_0001.wav
    seg_0002.wav
    ...
    manifest.json
```

`manifest.json`:

```json
{
  "clip_id": "…",
  "session_id": "…",
  "started_at_ms": 1714000000000,
  "ended_at_ms": 1714000300000,
  "segments": [
    { "id": "…", "start_ms": 1714000001500, "end_ms": 1714000004700, "file": "seg_0001.wav" },
    ...
  ]
}
```

After successful batch processing the manifest stays; the WAVs persist for the 14-day retention window so the archive UI can play them back.

### Schema deltas (one new SQL migration)

```sql
CREATE TABLE audio_clips (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES audio_sessions(id),
    started_at      INTEGER NOT NULL,
    ended_at        INTEGER NOT NULL,
    segment_count   INTEGER NOT NULL,
    manifest_path   TEXT NOT NULL,
    status          TEXT NOT NULL CHECK(status IN ('pending','running','processed','empty','failed')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    archive_model   TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);
CREATE INDEX idx_audio_clips_status ON audio_clips(status);

ALTER TABLE voiceprints ADD COLUMN kind TEXT NOT NULL DEFAULT 'enrolled'
    CHECK(kind IN ('enrolled','provisional'));
ALTER TABLE voiceprints ADD COLUMN provisional_last_matched_at INTEGER;

ALTER TABLE audio_segments ADD COLUMN clip_id TEXT REFERENCES audio_clips(id);
ALTER TABLE audio_segments ADD COLUMN clip_local_speaker_idx INTEGER;
```

Future widening of the `voiceprints.kind` CHECK would need the `_new → INSERT SELECT → DROP → RENAME` rebuild documented in `CLAUDE.md`. The initial `ADD COLUMN ... CHECK` here is fine because it does not modify an existing constraint.

`extraction_windows` rows are not migrated. The table stops receiving new rows; historical rows stay readable by the trace endpoint.

`reminders.source_window_id` is repurposed to reference `audio_clips.id` for new rows. Historical rows continue to reference `extraction_windows.id`. The trace endpoint (`GET /reminders/:id/trace`) is updated to look up the source ID in `audio_clips` first, falling back to `extraction_windows` for legacy rows. No SQL FK is declared on this column (it's a soft reference today as well).

### Settings additions

```rust
audio.live_asr_model: Option<String>          // defaults to current `asr_model`
audio.archive_asr_model: Option<String>       // defaults to a Whisper-medium variant
audio.audio_retention_days: u32               // default 14
audio.provisional_voiceprint_gc_days: u32     // default 30
audio.clip_target_secs: u32                   // default 300
audio.clip_max_secs: u32                      // default 360
audio.clip_close_silence_ms: u32              // default 1500
audio.cluster_cosine_threshold: f32           // default 0.4
```

Retiring: `audio.extraction_tick_secs`, `audio.window_length_ms`, `audio.window_step_ms`. The legacy `audio.asr_model` field migrates by populating `live_asr_model` and `archive_asr_model` on first read if either is `None`.

## Lifecycle scenarios

**Cold start, `always_listening = true`, unmuted:** supervisor starts CaptureDaemon and BatchProcessor. The user speaks; VAD segments are written to disk. The first clip closes after ~5 min. BatchProcessor wakes, loads archive ASR, transcribes, clusters, matches, writes transcripts + reminders, unloads ASR. Repeats indefinitely.

**User starts dictation:** supervisor starts LiveStreamingService. CaptureDaemon (already running) is told to also broadcast on the live channel. Live ASR loads in its own `spawn_blocking` worker. Live transcripts go to `/ws`. Audio still flows into clips on disk for the archive. When dictation ends, LiveStreamingService stops; live ASR unloads. CaptureDaemon and BatchProcessor are unaffected.

**User mutes mid-clip:** CaptureDaemon stops cpal. ClipBoundaryWatcher immediately closes the in-flight clip — even if shorter than the 5-min target — so the partial work is processed, not lost. On unmute, a fresh clip starts.

**`always_listening = false` (privacy mode):** supervisor does not start CaptureDaemon or BatchProcessor at boot. If the user starts dictation, supervisor spins up CaptureDaemon (live broadcast only — no disk writer) and LiveStreamingService. When dictation ends, both stop. No clips are ever archived in this mode.

**App crash mid-batch:** at startup, supervisor calls `requeue_stale_running` on `audio_clips`. Affected rows revert to `pending`; `attempts` increments. After three failed attempts the row is marked `failed` and tracing logs a warning.

**Two consecutive clips with the same unknown speaker:** clip 1's BatchProcessor runs first, AHC produces a cluster, no voiceprint match, an auto-provisional voiceprint row is inserted. Clip 2 runs second; the same speaker's centroid now matches the provisional row from clip 1; the unknown collapses to the same provisional speaker_id. Cross-clip coherence falls out of the existing matcher with no new linking infrastructure.

**LLM disabled:** `LlmRouterError::Disabled` reverts the post-clip extraction to `pending` without counting an attempt. Transcripts and clustering still persist (they don't depend on the LLM). When the user enables an LLM later, the existing window-claim logic processes the backlog.

## Edge cases & error handling

- **AHC degenerate input:** if `segment_count < 2` or total speech `< 5 s`, skip clustering, write segments without `clip_local_speaker_idx`, mark clip `empty`, skip the post-clip extractor.
- **Provisional voiceprint collisions:** two clips processed back-to-back may each create a provisional row for the same person if neither matches first. Mitigation: BatchProcessor is single-worker, so the second clip sees the first's row written. Documented behaviour, not a bug.
- **Live + batch use the same model file:** sherpa-onnx is `!Send`; each instance lives in its own `spawn_blocking` worker. Two sessions of the same ONNX file is fine — they're separate sherpa contexts.
- **Embedding dimension mismatch in clustering:** the existing per-row `embedding_dimension` rule applies. AHC operates only on the active model's dimension; never mixes 192-dim and 512-dim vectors.
- **Mute during BatchProcessor work:** capture stops, but in-flight batch processing finishes — the clip is already on disk. No abort path needed.
- **Disk full or WAV write failure:** CaptureDaemon logs and degrades gracefully — drops segments rather than crashing. The corresponding clip row is marked `failed` at boundary close.
- **Clip-close edge: pure silence past the 6-min cap:** force-close at the cap with whatever segments exist (including zero). Zero-segment clips short-circuit to `empty`.

## Testing

### Unit

- `cluster.rs::ahc` against synthetic embedding fixtures (separable clusters, near-collinear vectors, single-point input).
- `clip_boundary.rs` state machine driven by scripted VAD events: target-then-gap, target-then-no-gap-until-cap, mute-mid-clip, no-speech-at-all.
- `batch_processor.rs::process_clip` against a stubbed ASR + stubbed clustering — verifies row writes, attempt counting, post-clip hook firing.
- Provisional voiceprint GC sweep against a fixture DB with varied `provisional_last_matched_at` ages.

### Integration (in-memory SQLite, existing `actio-core` test harness)

- End-to-end "manifest written → transcripts + reminders persisted" using a `LlmRouter` stub. Verifies clip-local speaker indices, voiceprint matching, auto-provisional creation, attempt counting on failure, retention sweep.
- Cross-clip provisional linking: two clips with identical synthetic embeddings → second clip's centroid matches first's provisional row → both clips share `speaker_id`.
- Privacy-mode integration: `always_listening = false`, dictation start/stop cycle — no `audio_clips` rows produced, no `seg_*.wav` written.

### Manual smoke

Run `cargo run --bin actio-asr`, speak through a 7-min session with one known + one unknown speaker. Confirm:

1. Two clips on disk, both processed.
2. Second clip auto-links the unknown speaker to the provisional voiceprint created by the first.
3. Archive UI shows full 5-min transcripts with speaker labels.
4. Action items extracted with `reminders.source_window_id` resolving to the clip in `audio_clips`, viewable through the existing trace endpoint.
5. Memory after the session is idle: archive ASR is unloaded; live ASR was never loaded.

## Migration notes

- Old `extraction_windows` rows stay readable for the trace endpoint; no backfill required.
- Existing transcripts in the DB stay as-is; the new schema additions (`clip_id`, `clip_local_speaker_idx`) are nullable so historical rows are valid.
- On first boot after upgrade: the `voiceprints` migration sets `kind='enrolled'` for all existing rows. No user action needed.
- The `clips_dir` setting (currently used for per-failed-segment retention) repoints to the new per-clip directory structure. Old WAV files on disk become orphans; the generalized cleanup task sweeps them at 14 days.

## Open questions for plan-time

- Default value of `archive_asr_model` — pick a specific catalog entry once we have a quality benchmark on a sample 5-min clip.
- Default cosine threshold for AHC — start at 0.4 and tune empirically against test fixtures.
- Whether the live streaming path should be allowed to *contribute* to the provisional voiceprint pool (e.g., a strong embedding observed during dictation seeds a provisional row before any clip processes). Probably no for v1; revisit if archive coherence is poor in practice.
