# Repository Guidelines

## Project Structure & Module Organization
`src/` contains the Rust backend: `api/` for HTTP and WebSocket routes, `engine/` for orchestration and inference flow, `repository/` for database access, `domain/` for core types, and `grpc/` for generated service bindings. `python-worker/` hosts the gRPC inference worker and model-loading code. Database schema changes live in `migrations/`, protocol definitions in `proto/`, integration and unit tests in `tests/`, and the desktop shell in `src-tauri/`. Treat `target/` and `__pycache__/` as build artifacts, not source.

## Build, Test, and Development Commands
Use `docker compose up -d postgres` to start the local PostgreSQL instance on `localhost:5433`. Run `cargo run --bin actio-asr` to start the Rust API; it loads `.env`, applies migrations, and starts the Python worker. Use `cargo test` for the Rust test suite and `cargo fmt` to format Rust code before review. For the worker, install dependencies with `pip install -r python-worker/requirements.txt` or your preferred Python 3.12 environment, then run `python python-worker/main.py` when working on the worker in isolation. Build the desktop shell from `src-tauri/` with `cargo run`.

## Coding Style & Naming Conventions
Follow Rust defaults: 4-space indentation, `snake_case` for functions/modules, `PascalCase` for structs and enums, and small modules with explicit ownership. Keep SQLx, Axum, and Tokio code async-first and avoid blocking calls in request paths. Python code should also use 4-space indentation, `snake_case`, and focused services under `python-worker/services/`. Prefer `cargo fmt` for Rust formatting; keep Python formatting consistent with existing files.

## Testing Guidelines
Rust tests live in `tests/` and use descriptive names such as `test_circuit_breaker.rs` and `test_full_cycle`. Add tests next to the behavior you change, especially around engine state transitions, repository logic, and API contracts. Run `cargo test` before submitting changes. If you change migrations or worker startup behavior, verify the app boots cleanly against the local Postgres container.

## Commit & Pull Request Guidelines
This workspace no longer includes Git history, so there is no local commit convention to inspect. Use short, imperative commit messages such as `Add transcript aggregation retry`. Pull requests should include a concise summary, any required environment or migration notes, linked issues, and screenshots or logs when a UI or startup flow changes.

## Security & Configuration Tips
Configuration is loaded from `.env`. Do not commit real secrets, especially `LLM_API_KEY`. Keep local defaults aligned with the checked-in example values (`HTTP_PORT=3000`, `WORKER_PORT=50051`, Postgres on `5433`) unless the change is intentional and documented.
