<!-- Parent: ../AGENTS.md -->
<!-- Generated: 2026-04-10 | Updated: 2026-04-10 -->

# backend

## Purpose
The Rust backend is the core server for Actio. It exposes a REST + WebSocket API on port 3000, manages PostgreSQL persistence via SQLx, orchestrates the Python gRPC inference worker, and optionally generates todos/reminders using an LLM at session end.

## Key Files

| File | Description |
|------|-------------|
| `Cargo.toml` | Rust workspace/crate manifest; main binary is `actio-asr` |
| `Cargo.lock` | Pinned dependency versions |
| `build.rs` | Build script (proto compilation) |
| `docker-compose.yml` | Starts local PostgreSQL on port 5433 |
| `.env.example` | Template for required environment variables |
| `.env` | Local secrets — never commit real values |
| `design.md` | Architecture and design notes |
| `README.md` | Backend-specific quick-start |

## Subdirectories

| Directory | Purpose |
|-----------|---------|
| `src/` | Main Rust crate: API, engine, repository, domain, gRPC (see `src/AGENTS.md`) |
| `src-tauri/` | Tauri desktop shell crate (see `src-tauri/AGENTS.md`) |
| `proto/` | gRPC protobuf definitions for the inference worker (see `proto/AGENTS.md`) |
| `migrations/` | Ordered SQL schema migrations applied at startup (see `migrations/AGENTS.md`) |
| `python-worker/` | Python gRPC ML inference worker: ASR, VAD, speaker embedding (see `python-worker/AGENTS.md`) |
| `tests/` | Rust integration tests (see `tests/AGENTS.md`) |
| `docs/` | Planning documents and design specs |

## For AI Agents

### Working In This Directory
- Run all `cargo` commands from `backend/`, not the repo root.
- Start Postgres before running tests: `docker compose up -d postgres`
- Environment is loaded from `.env` via `dotenvy`; copy `.env.example` if missing.
- The binary entry point is `src/main.rs`; `AppState` wires together all engine components.
- `LLM_BASE_URL` / `LLM_API_KEY` are optional — omitting them disables todo generation silently.

### Testing Requirements
- `cargo test` — runs unit + integration tests
- `cargo fmt` — format before review
- Verify migrations apply cleanly: `cargo run --bin actio-asr` against a fresh DB

### Common Patterns
- Axum handlers receive `State<AppState>` — never pass mutable state via function arguments.
- All DB operations go through `repository/` using the `PgPool` stored in `AppState`.
- Circuit breaker (`engine/circuit_breaker.rs`) guards gRPC calls to the Python worker.
- `inference_router` in `AppState` is `Option<Arc<InferenceRouter>>` — always check for `None` (worker may be unavailable).

## Dependencies

### Internal
- `src/` depends on all other subdirectories
- `src-tauri/` depends on the HTTP API exposed by `src/`

### External
- `axum` — HTTP framework
- `sqlx` — async PostgreSQL driver with compile-time query checks
- `tokio` — async runtime
- `tonic` — gRPC client
- `utoipa` + `utoipa-swagger-ui` — OpenAPI docs at `/docs`
- `tracing` / `tracing-subscriber` — structured logging

<!-- MANUAL: -->
