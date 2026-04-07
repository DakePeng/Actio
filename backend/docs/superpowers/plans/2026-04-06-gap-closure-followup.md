# Gap Closure Follow-Up Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the remaining gaps between the current backend implementation and the existing April 5 plan documents, with priority on missing tests, transcript push/backfill behavior, metrics/health completeness, Python worker packaging, and LLM todo API conformance.

**Architecture:** Keep the current Rust service and Python worker structure intact. Finish the missing behaviors by extending the current aggregator/WebSocket flow, adding the missing test harness around the existing repository and session APIs, exposing worker/metrics state in existing endpoints, and tightening the todo API contract to match the original plan.

**Tech Stack:** Rust 1.94, Axum, Tokio, Tonic, SQLx/Postgres, Utoipa, Python 3.12+, grpcio/grpcio-tools

---

## File Structure

Files to create:

- `tests/common/mod.rs` for shared database/app bootstrap helpers used by repository and end-to-end tests
- `tests/test_repository.rs` for repository CRUD integration tests promised in Part 1
- `tests/test_e2e_session.rs` for HTTP/WebSocket session lifecycle coverage promised in Part 3
- `tests/integration/common.rs` if a second helper module is needed to keep test concerns separated
- `python-worker/inference_pb2.py` generated from `proto/inference.proto`
- `python-worker/inference_pb2_grpc.py` generated from `proto/inference.proto`

Files to modify:

- `src/engine/transcript_aggregator.rs` to add delayed speaker backfill and transcript event publishing
- `src/api/ws.rs` to push transcript updates back to clients instead of heartbeat-only behavior
- `src/engine/metrics.rs` to add worker/routing counters required by Part 3
- `src/api/mod.rs` to expose richer health information
- `src/main.rs` to wire any new broadcast/state handles into `AppState`
- `python-worker/health.py` to make worker health a real module instead of a placeholder comment
- `python-worker/main.py` to import/use the dedicated health module
- `src/api/session.rs` to return the planned todo response wrapper and use a 90s timeout in `end_session`
- `src/config.rs` to add explicit required LLM config loading alongside the existing optional mode, or replace optional mode if strict plan conformance is required
- `src/domain/types.rs` if a todo list response type is stored there rather than in the API module

---

### Task 1: Restore the missing repository and integration tests

**Files:**
- Create: `tests/common/mod.rs`
- Create: `tests/test_repository.rs`
- Create: `tests/test_e2e_session.rs`
- Create: `tests/integration/common.rs` if needed

- [ ] **Step 1: Add shared test bootstrap helpers**

Put DB/bootstrap helpers in `tests/common/mod.rs` so the repository and e2e tests stop duplicating setup logic:

```rust
pub async fn test_pool() -> sqlx::PgPool {
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost:5433/actio_asr_test".to_string());
    let pool = actio_asr::repository::db::create_pool(&database_url).await.unwrap();
    actio_asr::repository::db::run_migrations(&pool).await.unwrap();
    pool
}
```

- [ ] **Step 2: Re-add the repository integration tests the plan claims exist**

Create `tests/test_repository.rs` with CRUD coverage for speaker, session, transcript, and todo persistence using the current repository signatures:

```rust
#[tokio::test]
async fn test_session_transcript_roundtrip() {
    let pool = common::test_pool().await;
    let session = actio_asr::repository::session::create_session(
        &pool,
        uuid::Uuid::nil(),
        "microphone",
        "realtime",
    ).await.unwrap();

    let created = actio_asr::repository::transcript::create_transcript(
        &pool,
        session.id,
        "hello world",
        0,
        600,
        false,
        None,
    ).await.unwrap();

    let rows = actio_asr::repository::transcript::get_transcripts_for_session(&pool, session.id)
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, created.id);
}
```

- [ ] **Step 3: Add an end-to-end session/API test**

Create `tests/test_e2e_session.rs` to verify at least this sequence:

```rust
1. POST /sessions creates a session
2. POST /sessions/{id}/end returns 204
3. GET /sessions/{id} shows ended_at is set
4. GET /sessions/{id}/todos returns the new wrapper shape
```

Use an in-process axum router built from `src/api/mod.rs` and a real test DB.

- [ ] **Step 4: Run the promised tests**

Run:

```bash
cargo test --test test_repository -- --nocapture
cargo test --test test_e2e_session -- --nocapture
```

Expected: both test binaries pass.

- [ ] **Step 5: Commit**

```bash
git add tests/common/mod.rs tests/test_repository.rs tests/test_e2e_session.rs tests/integration/common.rs
git commit -m "test: restore repository and end-to-end session coverage"
```

---

### Task 2: Finish transcript backfill and real-time WebSocket push

**Files:**
- Modify: `src/engine/transcript_aggregator.rs`
- Modify: `src/api/ws.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Extend the aggregator with backfill and event publishing**

Add a broadcast channel to `TranscriptAggregator` so the WebSocket layer can subscribe to transcript updates:

```rust
pub struct TranscriptAggregator {
    pool: sqlx::PgPool,
    events: tokio::sync::broadcast::Sender<AggregatedTranscript>,
}
```

Expose a subscriber method:

```rust
pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AggregatedTranscript> {
    self.events.subscribe()
}
```

Implement the missing delayed speaker backfill API:

```rust
pub async fn backfill_speaker(
    &self,
    transcript_id: Uuid,
    speaker_id: Uuid,
) -> Result<AggregatedTranscript, sqlx::Error> {
    let transcript = sqlx::query!(
        r#"SELECT start_ms, end_ms, text FROM transcripts WHERE id = $1"#,
        transcript_id
    )
    .fetch_one(&self.pool)
    .await?;

    self.finalize(transcript_id, &transcript.text, Some(speaker_id)).await
}
```

Every successful `add_partial`, `add_final`, `finalize`, and `backfill_speaker` call should publish the resulting `AggregatedTranscript` to `events`.

- [ ] **Step 2: Push transcript updates over the WebSocket**

Modify `src/api/ws.rs` so the sender task subscribes to aggregator events and sends JSON messages to the client:

```rust
#[derive(serde::Serialize)]
struct WsTranscriptEvent {
    kind: &'static str,
    transcript_id: uuid::Uuid,
    text: String,
    start_ms: i64,
    end_ms: i64,
    is_final: bool,
    speaker_id: Option<uuid::Uuid>,
}
```

Use `Message::Text` with serialized JSON instead of ping-only output. Keep heartbeat pings, but do not make them the only outbound traffic.

- [ ] **Step 3: Add a regression test for transcript event delivery**

Either in `tests/test_e2e_session.rs` or a dedicated `tests/test_ws_session.rs`, cover this behavior:

```rust
1. Open /ws
2. Send one binary audio chunk
3. Inject or trigger an aggregator transcript event
4. Assert the socket receives a JSON transcript update
```

- [ ] **Step 4: Run targeted verification**

Run:

```bash
cargo test transcript_aggregator -- --nocapture
cargo test test_e2e_session -- --nocapture
```

Expected: transcript push and backfill behavior are covered by tests.

- [ ] **Step 5: Commit**

```bash
git add src/engine/transcript_aggregator.rs src/api/ws.rs src/main.rs tests/
git commit -m "feat: add transcript backfill and websocket transcript push"
```

---

### Task 3: Complete health and metrics reporting

**Files:**
- Modify: `src/engine/metrics.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs`
- Modify: `python-worker/health.py`
- Modify: `python-worker/main.py`

- [ ] **Step 1: Add the missing metrics fields**

Extend `src/engine/metrics.rs` with the counters implied by the original plan:

```rust
pub struct Metrics {
    pub active_sessions: AtomicU32,
    pub total_chunks_received: AtomicU64,
    pub unknown_speaker_count: AtomicU64,
    pub local_route_count: AtomicU64,
    pub worker_error_count: AtomicU64,
    pub transcript_push_count: AtomicU64,
    pub start_time: Instant,
}
```

Add a health response model that includes worker state:

```rust
#[derive(Serialize)]
pub struct HealthSummary {
    pub active_sessions: u32,
    pub uptime_secs: u64,
    pub worker_state: String,
}
```

- [ ] **Step 2: Surface real health data from `/health`**

Update `src/api/mod.rs` so `/health` returns `Json<HealthSummary>` instead of a static string/object. Derive `worker_state` from `AppState`, for example:

```rust
let worker_state = if state.inference_router.is_some() {
    "available"
} else {
    "degraded"
}.to_string();
```

This does not need to be perfect worker introspection in the first pass, but it must expose a worker-state field rather than omitting it.

- [ ] **Step 3: Turn `python-worker/health.py` into a real module**

Replace the placeholder with a reusable helper:

```python
from grpc_health.v1 import health, health_pb2

def build_health_servicer() -> health.HealthServicer:
    servicer = health.HealthServicer()
    servicer.set("", health_pb2.HealthCheckResponse.SERVING)
    return servicer
```

Import that helper from `python-worker/main.py` instead of building health state inline.

- [ ] **Step 4: Verify health output**

Run:

```bash
cargo check
cargo run
curl http://localhost:3000/health
```

Expected: the JSON includes `active_sessions`, `uptime_secs`, and `worker_state`.

- [ ] **Step 5: Commit**

```bash
git add src/engine/metrics.rs src/api/mod.rs src/main.rs python-worker/health.py python-worker/main.py
git commit -m "feat: complete health response and metrics surface"
```

---

### Task 4: Restore Python gRPC generated artifacts

**Files:**
- Create: `python-worker/inference_pb2.py`
- Create: `python-worker/inference_pb2_grpc.py`

- [ ] **Step 1: Generate the Python gRPC files from the checked-in proto**

Run:

```bash
python -m grpc_tools.protoc -I proto --python_out=python-worker --grpc_python_out=python-worker proto/inference.proto
```

Expected:

```bash
python-worker/inference_pb2.py
python-worker/inference_pb2_grpc.py
```

- [ ] **Step 2: Verify the worker imports from generated code**

Run:

```bash
python -c "import sys; sys.path.insert(0, 'python-worker'); import inference_pb2, inference_pb2_grpc; print('grpc stubs ok')"
```

Expected: `grpc stubs ok`

- [ ] **Step 3: Commit**

```bash
git add python-worker/inference_pb2.py python-worker/inference_pb2_grpc.py
git commit -m "build: check in generated python gRPC stubs"
```

---

### Task 5: Bring the LLM todo API back to the planned contract

**Files:**
- Modify: `src/api/session.rs`
- Modify: `src/config.rs`
- Modify: `src/domain/types.rs` or `src/api/session.rs`
- Modify: `src/engine/todo_generator.rs` if extra generated-state handling is needed

- [ ] **Step 1: Return the planned response wrapper from GET todos**

Add a response type:

```rust
#[derive(Serialize, ToSchema)]
pub struct TodoListResponse {
    pub todos: Vec<TodoItem>,
    pub generated: bool,
}
```

Change `get_todo_items` to return:

```rust
Ok(Json(TodoListResponse {
    generated: true,
    todos,
}))
```

- [ ] **Step 2: Add the missing 90-second timeout around todo generation**

Wrap the spawned `generate_session_todos` work in `tokio::time::timeout` inside `end_session`:

```rust
let result = tokio::time::timeout(
    std::time::Duration::from_secs(90),
    todo_generator::generate_session_todos(&pool, &llm_client, id, tenant_id),
).await;
```

Log timeout separately from normal generator errors.

- [ ] **Step 3: Decide and implement strict vs optional LLM config**

Choose one of these and document it in code:

```rust
Option A: keep from_env_optional() for local dev, add from_env() for strict plan conformance
Option B: replace optional loading with from_env() and fail startup when LLM is required
```

For minimal churn, prefer Option A and update startup code to call the strict variant only in environments that require todo generation.

- [ ] **Step 4: Add API coverage for the todo route**

Add a test asserting:

```rust
GET /sessions/{id}/todos returns:
{
  "todos": [],
  "generated": true
}
```

- [ ] **Step 5: Run full verification**

Run:

```bash
cargo test
cargo clippy
```

Expected: tests pass and clippy introduces no new issues from the gap-closure work.

- [ ] **Step 6: Commit**

```bash
git add src/api/session.rs src/config.rs src/domain/types.rs src/engine/todo_generator.rs tests/
git commit -m "feat: align todo API and timeout behavior with plan"
```

---

## Execution Order

```text
Lane A:
  Task 4 -> Task 1 -> Task 2 -> Task 3 -> Task 5

Why:
  - Task 4 restores generated Python artifacts immediately and removes a packaging gap.
  - Task 1 restores the missing test harness before deeper behavioral changes.
  - Task 2 depends on test coverage and current aggregator/ws code.
  - Task 3 is mostly independent but touches shared AppState/health behavior.
  - Task 5 should land after health/tests because it changes public API contracts and final verification.
```

## Completion Checklist

- [ ] `python-worker/inference_pb2.py` exists in the repo
- [ ] `python-worker/inference_pb2_grpc.py` exists in the repo
- [ ] `tests/test_repository.rs` exists and passes
- [ ] `tests/test_e2e_session.rs` exists and passes
- [ ] transcript updates are pushed to WebSocket clients
- [ ] transcript backfill behavior exists in `TranscriptAggregator`
- [ ] `/health` returns `worker_state`
- [ ] metrics include more than the original four counters
- [ ] `GET /sessions/{id}/todos` returns `{ "todos": [...], "generated": true }`
- [ ] `end_session` uses a 90 second timeout for todo generation
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes
