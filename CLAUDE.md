# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repo Shape

Monorepo with **two independent projects**:

- `backend/` — Rust workspace (`actio-core` library + `actio-asr` bin + `src-tauri` desktop shell) running Axum HTTP on port **3000**, SQLite via SQLx, and all ML inference in-process via `sherpa-onnx`. Edition 2021.
- `frontend/` — React 19 + Vite + TypeScript + Zustand + Tailwind. Dev server on **5173**, optional `json-server` mock reminder API on **3001**. Tauri v2 shell wraps the built backend binary.

There is **no** Python worker, no gRPC, no Postgres, no Docker. Older docs mentioning those are obsolete.

`AGENTS.md` files are placed in almost every directory and contain more targeted guidance — read the nearest one when working on a specific area. `frontend/CLAUDE.md` has extra frontend-specific conventions (pnpm-only; don't introduce Bun runtime deps despite the hint to default to Bun for scripts).

## Commands

Always `cd` into the subproject first — `cargo`/`pnpm` run from root will not find manifests.

### Backend (`cd backend`)

```bash
cargo run --bin actio-asr           # start server on :3000 (creates SQLite DB on first run)
cargo check -p actio-core --tests   # fast compile check
cargo test  -p actio-core --lib     # unit tests (in-memory SQLite)
cargo test  -p actio-core --lib <name_substring>   # run tests matching substring
cargo fmt
cargo clippy
```

`cargo check`/`test` trigger linking against `sherpa-onnx` (large), so allow a few minutes on a cold build. `[profile.dev.package."*"] opt-level = 3` in `Cargo.toml` is intentional — removing it makes CPU inference 50–100× slower.

### Frontend (`cd frontend`)

```bash
pnpm install
pnpm dev                # Vite web dev @ :5173
pnpm mock:api           # json-server @ :3001 (UI-only dev, no backend needed)
pnpm build              # tsc -b + vite build
pnpm test               # vitest run
pnpm test -t "substring"                 # filter by test name
pnpm exec vitest run src/path/file.test.ts   # single file
pnpm tsc --noEmit       # typecheck only
pnpm tauri:dev          # full desktop shell (builds backend Tauri crate)
```

Tests live in `src/**/__tests__/` next to their subjects. The `i18n/__tests__/parity.test.ts` file enforces `Object.keys(en) === Object.keys(zh-CN)` — every new locale key must land in both files.

## Architecture

### Audio & inference pipeline (`backend/actio-core/src/engine/`)

Two implementations live side by side, gated by `audio.use_batch_pipeline` (default **true**). They are mutually exclusive — both would grab the microphone.

**`sherpa-onnx` is `!Send`** for both pipelines. Wrap each extractor/recognizer's entire lifecycle in a single `tokio::task::spawn_blocking` (or a plain `std::thread` for long-lived workers) and bridge with `mpsc`/`oneshot`/`crossbeam_channel`. See `diarization.rs::EMBEDDING_WORKERS` — per-model worker threads cached in an LRU-capped registry (size 2) so model swaps don't leak ONNX speaker-embedding models (~30–70MB each).

#### Legacy InferencePipeline (`use_batch_pipeline = false`)

`InferencePipeline` reads 16 kHz mono via cpal → Silero VAD → ASR (Zipformer / Whisper / SenseVoice / FunASR / Moonshine, selected per-session) → speaker embedding → `ContinuityState` → broadcasts `transcript` and `speaker_resolved` frames on `/ws`. Persistence (transcripts, segments, speakers) is synchronous against SQLite.

`AppState::pipeline_supervisor` spawns it at boot. With `always_listening = true` it stays up; with `always_listening = false` the supervisor hibernates after `IDLE_GRACE_PERIOD` of no WS subscribers and wakes on next connect.

Dictation and live voiceprint enrollment still call `InferencePipeline::start_session` regardless of the flag — flipping those to `LiveStreamingService` is the last unfinished migration step. Translation is a separate path (see LLM router section below) and never enters the audio pipeline.

### Batch clip processing pipeline (default, `audio.use_batch_pipeline = true`)

Components in `engine/`:

- **`capture_daemon.rs`** — long-lived cpal + Silero VAD producer; broadcasts `CaptureEvent::{Speech | Muted | Unmuted}` on a tokio broadcast channel. `archive_enabled` flag (driven from `always_listening` by the supervisor) gates whether the clip writer persists; live subscribers receive events regardless.
- **`clip_writer.rs`** — subscribes to the daemon, runs `clip_boundary` state machine (close on first ≥1.5 s silence after the 5-min target, hard-cap 6 min, immediate-close on mute), writes per-VAD-segment WAVs under `<clips_dir>/<session_id>/<clip_id>/seg_NNNN.wav` plus a `manifest.json`, and inserts the matching `audio_clips` row.
- **`batch_processor.rs`** — single-worker claim loop. `process_clip_production` runs offline ASR over every segment in a manifest, embeds each segment via `diarization::extract_embedding`, AHC-clusters the embeddings, matches centroids against `speakers` (enrolled rows from `speaker_embeddings`, provisional rows aggregated from `audio_segments.embedding`), reuses an existing speaker or inserts a fresh provisional one, and assigns `speaker_id` + `clip_local_speaker_idx` to every segment. Then calls `window_extractor::extract_for_clip` for action items.
- **`live_streaming.rs`** — on-demand service for dictation/translation that subscribes to the same daemon, runs offline ASR per segment, broadcasts on `/ws` via `TranscriptAggregator::broadcast_*` without DB writes. Streaming Zipformer is intentionally not supported (consumes raw chunks, not VAD segments).

`speakers.kind` (`'enrolled' | 'provisional'`) and `speakers.provisional_last_matched_at` were added in migration 005. Provisional rows surface in the **Candidate Speakers panel** (`/candidate-speakers` API + People-tab UI section); promote renames + flips kind, dismiss hard-deletes (segments lose `speaker_id` via existing FK).

`reminders.source_window_id` no longer FKs to `extraction_windows` (migration 006 dropped the constraint). The `/reminders/:id/trace` endpoint resolves the source ID against `audio_clips` first, falling back to `extraction_windows` for legacy rows.

The flag is mutually exclusive with the legacy `InferencePipeline` because both grab the microphone. Toggling it requires a restart. Live enrollment + dictation handlers in `api/session.rs` still call `InferencePipeline::start_session` (`api/session.rs:68` and `:680`) — flipping those over is the last unfinished migration step. `api/translate.rs` is unrelated to this migration: it's a stateless `POST /llm/translate` handler that calls `LlmRouter::translate_lines` directly, with no audio capture or session lifecycle.

### Speaker continuity (`engine/continuity.rs`)

Pure state machine: `MatchEvidence { Confirmed | Tentative | Unknown }` → `(AttributionOutcome, new ContinuityState)`. Notable rules — a **single** tentative match for a different speaker does not flip state when there's a carry-over in progress; two consecutive tentatives for the same candidate, or any confirmed, are required. Out-of-order segments (now possible because the cached embedding-worker pool can resolve segments out of order) are rejected explicitly in `within_window`.

### Always-listening action extractor (`engine/window_extractor.rs`)

Rolling 5-min windows (4-min step → 1-min overlap by default, all settings-driven). `schedule_windows_for_active_sessions` enumerates candidate windows per `audio_sessions WHERE ended_at IS NULL` up to `MAX(transcripts.end_ms) - 30s`. `claim_next_pending` is an atomic `UPDATE … RETURNING` so one LLM call at a time runs process-wide. Each window's transcripts+segments+speakers are joined into a `[HH:MM:SS • Speaker]: text` prompt, sent to `LlmRouter::generate_action_items_with_refs`, then items are **confidence-gated**:

- `confidence === "high"` → `status='open'` (lands on the Board)
- `confidence === "medium"` → `status='pending'` (Needs-review queue)
- anything else → dropped

Every saved reminder carries `source_window_id` so `GET /reminders/:id/trace` can render a "Show context" inspector.

### LLM router (`engine/llm_router.rs`)

Three-way fan-out: `Disabled`, `Local { slot, model_id }` (llama-cpp-2, gated by default `local-llm` Cargo feature), `Remote(RemoteLlmClient)` (OpenAI-compatible HTTP). `LlmRouterError::Disabled` is **not** a failure — the window extractor catches it and reverts the claimed window to `pending` without counting an attempt, so the user can enable an LLM later without leaving orphan `failed` rows.

Translation (`POST /llm/translate`, `api/translate.rs`) shares this router via `LlmRouter::translate_lines`. It serializes against window-extractor calls through `state.llm_inflight` so a long extraction doesn't block translation indefinitely (and vice versa). When the router is in `Disabled` mode, the endpoint returns `503 {"error":"llm_disabled"}` so the frontend can surface a precise toast.

### Live voiceprint enrollment (`engine/live_enrollment.rs`, `api/session.rs`)

Enroll = 5 passages × ~5s read aloud. While `EnrollmentState::Active`, `consume_segment` routes VAD segments to the speaker's voiceprints instead of the normal identify+candidate path. Gate checks happen **inside** the Mutex critical section to avoid snapshot-recheck races. Cancelling cleans **only** the rows saved during the current session via `cleanup_partial_embeddings` — prior successful enrollments for the same speaker survive. A watchdog tokio task owns natural-completion teardown (pipeline stop + `session::end_session`) so a Complete status doesn't leak an unbounded DB session.

### Frontend stores (`frontend/src/store/`)

- `use-store.ts` — reminders, labels, filter, UI. `filterReminders` excludes `status==='pending'`; `pendingReminders()` is the selector for the Needs-review tab.
- `use-voice-store.ts` — live transcript, segments, **and** the speakers registry (not in `use-store`). Contains a module-level `pendingResolutions` buffer that replays `speaker_resolved` events against lines that finalize **after** the event arrives — fixes "identifying forever" on short utterances.

Both stores talk to the backend through `api/actio-api.ts` (`createActioApiClient()`) and `api/speakers.ts`.

### Tauri windowing (`frontend/src/components/StandbyTray.tsx` + `BoardWindow.tsx`)

Two modes — collapsed tray (320 px) and expanded/board (440 px and up). The Tauri window itself is resized via `invoke('sync_window_mode', …)`; there are three **critical skips** documented in `StandbyTray.tsx` to avoid stomping the exit animation and the saved-position read. Do not simplify these guards without reading the comments.

### i18n (`frontend/src/i18n/`)

Custom ~40-line provider (no `react-i18next`). `TKey = keyof typeof en`; `Translations = Record<TKey, string>` so zh-CN values widen to `string` and don't trigger literal-type errors. `label-names.ts` maps the six seeded default label names (backend-stored English strings) to localised keys so translations work without a DB migration.

## Ports

| Service | Port |
|---|---|
| Backend HTTP/WS | 3000 |
| Frontend Vite dev | 5173 |
| Mock reminder API (`pnpm mock:api`) | 3001 |
| Local LLM endpoint (OpenAI-compatible shim, optional) | 3001 configurable via settings |

The mock-API and local-LLM ports collide by default — pick one when doing UI-only work.

## Non-obvious patterns

- **Embedding dimension is per-model, not a single repo-wide constant.** Five of six catalog models (CAM++ family + ERes2Net v2 + TitaNet) emit 192-dim vectors; ERes2Net Base emits 512-dim. The DB tracks `embedding_dimension` per row and `speaker_matcher` filters joins on the active dim, so cross-model rows are silently ignored. Production INSERT sites already pass `embedding.len()` — never hardcode either number outside test fixtures.
- **SQLite `ALTER TABLE` can't change CHECK constraints.** Migrations that need to widen an enum (e.g. `reminders.status` gaining `'pending'` in `004_action_windows.sql`) rebuild the table via `CREATE _new → INSERT SELECT → DROP → RENAME → recreate indexes`.
- **`NewReminder` derives `Default`.** Any new caller should still spell out every field for grep-ability; only reach for `..Default::default()` when the struct has grown > ~10 fields.
- **Hooks lie sometimes.** `PostToolUse:Edit` may say "Edit operation failed" when the body reports success. Trust the body.
- **`settings-check` is an opt-in class.** `input[type="checkbox"]` globally is unstyled; add `className="settings-check"` (and `role="switch" aria-checked={v}`) for the iOS-pill toggle.
- **Provisional speakers are gated, not created per AHC cluster.** `audio.cluster_min_segments` (default 3) and `audio.cluster_min_duration_ms` (default 8000) AND-gate cluster → speaker creation in `batch_processor::cluster_passes_gate`. Both `process_clip_with_clustering` (test path) and `process_clip_production` (sherpa path) honour them, so semantics stay aligned. Defaults exist to suppress noise / mic blips / podcast cameos from flooding the People → Candidate Speakers panel; lowering them is fine for synthetic-cluster tests but will reintroduce the flood in production.

## OpenAPI

Full request/response schemas live at `http://localhost:3000/docs` (utoipa-swagger-ui) while the backend is running.
