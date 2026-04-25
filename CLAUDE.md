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

`InferencePipeline` is the runtime heart. `AppState::pipeline_supervisor` spawns it at boot and keeps it alive:

- **With `always_listening = true` (default):** the pipeline stays up as long as the process is running. This is required for the background action extractor to have transcripts to read.
- **With `always_listening = false`:** legacy hibernation — the supervisor stops the pipeline after `IDLE_GRACE_PERIOD` of no WebSocket subscribers, and wakes it again on the next connect.

The pipeline reads 16 kHz mono via cpal → Silero VAD → ASR (Zipformer / Whisper / SenseVoice / FunASR / Moonshine, selected per-session) → speaker embedding → `ContinuityState` → broadcasts `transcript` and `speaker_resolved` frames on `/ws`. Persistence (transcripts, segments, speakers) is synchronous against SQLite.

**`sherpa-onnx` is `!Send`.** Wrap each extractor/recognizer's entire lifecycle in a single `tokio::task::spawn_blocking` (or a plain `std::thread` for long-lived workers) and bridge with `mpsc`/`oneshot`/`crossbeam_channel`. See `diarization.rs::EMBEDDING_WORKERS` — per-model worker threads cached in an LRU-capped registry (size 2) so model swaps don't leak ONNX speaker-embedding models (~30–70MB each).

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

## OpenAPI

Full request/response schemas live at `http://localhost:3000/docs` (utoipa-swagger-ui) while the backend is running.
