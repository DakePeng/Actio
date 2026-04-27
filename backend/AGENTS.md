<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-10 | Updated: 2026-04-27 -->

# backend

## Purpose
The Rust backend is the core server for Actio. It exposes a REST + WebSocket API on port 3000 (with frontend-side fallback to 3001–3009), manages SQLite persistence via SQLx, runs all ML inference in-process via `sherpa-onnx` (VAD, ASR, speaker embedding, diarization), and generates reminders from transcripts on a rolling-window cadence — `engine::window_extractor` slices the active session into 5-min windows (4-min step), routes each to an LLM (local llama.cpp or remote OpenAI-compatible), and confidence-gates the items into either the Board (`status='open'`) or the Needs-Review queue (`status='pending'`). There is no Python worker and no gRPC.

## Key Files

| File | Description |
|------|-------------|
| `Cargo.toml` | Workspace manifest (members: `actio-core`, `src-tauri`) |
| `Cargo.lock` | Pinned dependency versions |
| `README.md` | Backend-specific quick-start and API reference |
| `design.md` | Current architecture and design notes |
| `.env.example` | Template for optional LLM environment variables |
| `.env` | Local secrets — never commit real values |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `actio-core/` | Main library crate: api, engine, repository, domain (binary: `actio-asr`) |
| `src-tauri/` | Tauri desktop shell crate (see `src-tauri/AGENTS.md`) |
| `migrations/` | Ordered SQL schema migrations applied at startup (see `migrations/AGENTS.md`) |
| `docs/` | Planning documents and historical design specs |

## For AI Agents

### Working In This Directory
- Run all `cargo` commands from `backend/`, not the repo root.
- SQLite database file is created next to the binary on first run — no external DB setup required.
- Environment is loaded from `.env` via `dotenvy`; it is optional.
- The binary entry point is in `actio-core` with `AppState` wiring engine components.
- `LLM_BASE_URL` / `LLM_API_KEY` are optional — omitting them disables remote reminder extraction silently; the default `local-llm` Cargo feature bundles llama.cpp for on-device inference.

### Testing Requirements
- `cargo test` — runs unit + integration tests (no external services required)
- `cargo fmt` — format before review
- `cargo clippy` — lint

### Common Patterns
- Axum handlers receive `State<AppState>` — never pass mutable state via function arguments.
- All DB operations go through `repository/` using a `SqlitePool` stored in `AppState`.
- `sherpa-onnx` types like `OnlineRecognizer` and `VoiceActivityDetector` hold raw pointers and are `!Send` — wrap their entire lifecycle in a single `tokio::task::spawn_blocking` and bridge with `crossbeam_channel`.
- **Speaker embedding dimension is per-model, not a single repo-wide constant.** CAM++ family + ERes2Net v2 + TitaNet (5 of 6 catalog models) emit **192-dim** vectors; only ERes2Net Base emits **512-dim**. The `speaker_embeddings` table stores `embedding_dimension` per row and `speaker_matcher` filters joins on the active dim, so cross-dim rows are silently ignored. Production INSERT sites already pass `embedding.len()` — never hardcode either number outside test fixtures.

## Dependencies

### Internal
- `actio-core/` depends on nothing else in the workspace
- `src-tauri/` depends on the HTTP API exposed by `actio-core/`

### External
- `axum` — HTTP framework
- `sqlx` (SQLite feature) — async database driver
- `tokio` — async runtime
- `sherpa-onnx` — embedded ONNX Runtime for VAD/ASR/speaker/diarization
- `llama-cpp-2` (optional, default feature) — local GGUF LLM inference
- `reqwest` — HTTP client for remote LLM + model downloads
- `utoipa` + `utoipa-swagger-ui` — OpenAPI docs at `/docs`
- `tracing` / `tracing-subscriber` — structured logging

<!-- MANUAL: -->
