<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-10 | Updated: 2026-04-17 -->

# backend

## Purpose
The Rust backend is the core server for Actio. It exposes a REST + WebSocket API on port 3000, manages SQLite persistence via SQLx, runs all ML inference in-process via `sherpa-onnx` (VAD, ASR, speaker embedding, diarization), and optionally generates reminders from transcripts using a local (llama.cpp) or remote (OpenAI-compatible) LLM at session end. There is no Python worker and no gRPC.

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
| `tests/` | Rust integration tests (see `tests/AGENTS.md`) |
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
- Speaker embeddings are 512-dim (3D-Speaker). Any hardcoded `192` in the code is stale from the earlier CAM++ design and should be fixed as you encounter it.

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
