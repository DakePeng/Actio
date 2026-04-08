# Actio ASR Backend

Real-time speech recognition and speaker identification service. A Rust HTTP/WebSocket server controls a Python gRPC worker that runs local ASR (FunASR Paraformer) and speaker embedding (CAM++) models. Transcripts are stored in PostgreSQL and action items are extracted from completed sessions via an LLM.

## Architecture

```
Client (audio)
    │  WebSocket /ws
    ▼
Rust Service (Axum)
    │  gRPC (tonic)
    ▼
Python Worker (grpcio)
    ├── VAD  — energy-based speech detection
    ├── ASR  — FunASR Paraformer-Streaming (16kHz/mono/16-bit PCM, 600ms chunks)
    └── Speaker — CAM++ 192-dim embeddings + cosine similarity + Z-Norm
    
PostgreSQL + pgvector
    └── sessions, speakers, speaker_embeddings, audio_segments, transcripts, todos
```

**Key design decisions:**
- Audio: 16 kHz / 16-bit / mono PCM, 600 ms chunks
- Speaker identification: CAM++ embeddings, Z-Norm threshold 0.0
- Circuit breaker: 3-state (Closed → Open after 3 failures → HalfOpen after 30 s)
- Speaker tag delay: transcripts emit as `[UNKNOWN]`, backfilled ~2 s later
- LLM todo generation: triggered on session end, 90 s timeout, idempotent

## Prerequisites

- Rust 1.82+
- Python 3.12.x with [uv](https://docs.astral.sh/uv/)
- PostgreSQL with `pgvector` extension
- `protoc` (for regenerating gRPC stubs)

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | yes | — | Postgres connection string |
| `HTTP_PORT` | no | `3000` | HTTP server port |
| `WORKER_HOST` | no | `127.0.0.1` | Python worker host |
| `WORKER_PORT` | no | `50051` | Python worker gRPC port |
| `LLM_BASE_URL` | no | — | OpenAI-compatible endpoint (enables todo generation) |
| `LLM_API_KEY` | no | — | API key for LLM |
| `LLM_MODEL` | no | `gpt-4o-mini` | Model name |

Copy `.env.example` if present, or create a `.env`:

```env
DATABASE_URL=postgres://actio:actio@localhost:5432/actio
LLM_BASE_URL=https://api.openai.com/v1
LLM_API_KEY=sk-...
```

## Running

### 1. Start PostgreSQL

Ensure the `pgvector` extension is available:

```sql
CREATE EXTENSION IF NOT EXISTS vector;
```

### 2. Start the Python worker

```bash
cd python-worker
uv sync
uv run python main.py
```

Models (FunASR Paraformer, CAM++) are downloaded from ModelScope on first run.

### 3. Start the Rust service

```bash
cargo run
```

Migrations run automatically on startup. The service starts on `http://0.0.0.0:3000`.

## API Endpoints

Interactive docs are available at `http://localhost:3000/docs` (Swagger UI) and the raw OpenAPI schema at `GET /api-docs/openapi.json`.

---

### Health

#### `GET /health`

Returns service health and runtime metrics.

**Response `200`**
```json
{
  "active_sessions": 2,
  "uptime_secs": 3600,
  "worker_state": "available",
  "local_route_count": 142,
  "worker_error_count": 0,
  "unknown_speaker_count": 5
}
```

`worker_state` is `"available"` when the Python worker gRPC connection is up, `"degraded"` otherwise.

---

### Sessions

#### `POST /sessions`

Start a new audio session.

**Headers:** `x-tenant-id: <uuid>` (optional, falls back to nil UUID)

**Request body**
```json
{
  "tenant_id": "00000000-0000-0000-0000-000000000001",
  "source_type": "microphone",
  "mode": "realtime"
}
```

All fields are optional. `source_type` defaults to `"microphone"`, `mode` to `"realtime"`.

**Response `201`**
```json
{
  "id": "d290f1ee-6c54-4b01-90e6-d701748f0851",
  "started_at": "2026-04-08T10:00:00Z"
}
```

---

#### `GET /sessions/{id}`

Fetch session details.

**Response `200`**
```json
{
  "id": "d290f1ee-6c54-4b01-90e6-d701748f0851",
  "tenant_id": "00000000-0000-0000-0000-000000000001",
  "source_type": "microphone",
  "mode": "realtime",
  "routing_policy": "local",
  "started_at": "2026-04-08T10:00:00Z",
  "ended_at": null,
  "metadata": {}
}
```

---

#### `POST /sessions/{id}/end`

End a session. Triggers LLM todo generation in the background (90 s timeout) if `LLM_BASE_URL` is configured. Idempotent — calling it twice is safe.

**Response `204 No Content`**

---

#### `GET /sessions/{id}/transcripts`

List all transcripts for a session, ordered by creation time.

**Response `200`**
```json
[
  {
    "id": "a1b2c3d4-...",
    "session_id": "d290f1ee-...",
    "segment_id": null,
    "start_ms": 0,
    "end_ms": 600,
    "text": "[UNKNOWN] Hello, let's get started.",
    "is_final": true,
    "backend_type": "local",
    "created_at": "2026-04-08T10:00:01Z"
  }
]
```

---

#### `GET /sessions/{id}/todos`

List action items extracted from the session transcript. Returns an empty list if the session has not ended or LLM generation has not completed yet.

**Response `200`**
```json
{
  "todos": [
    {
      "id": "f47ac10b-...",
      "session_id": "d290f1ee-...",
      "speaker_id": null,
      "assigned_to": "Alice",
      "description": "Send the budget report by Friday",
      "status": "open",
      "priority": "high",
      "created_at": "2026-04-08T10:05:00Z",
      "updated_at": "2026-04-08T10:05:00Z"
    }
  ],
  "generated": true
}
```

`status` values: `open`, `completed`, `archived`  
`priority` values: `high`, `medium`, `low`, or `null`

---

### Speakers

#### `POST /speakers`

Register a speaker for identification.

**Headers:** `x-tenant-id: <uuid>` (optional)

**Request body**
```json
{
  "display_name": "Alice"
}
```

**Response `201`**
```json
{
  "id": "7c9e6679-...",
  "tenant_id": "00000000-...",
  "display_name": "Alice",
  "status": "active",
  "created_at": "2026-04-08T09:00:00Z"
}
```

---

#### `GET /speakers`

List all speakers for the tenant.

**Headers:** `x-tenant-id: <uuid>` (optional)

**Response `200`** — array of Speaker objects (same shape as POST response)

---

### WebSocket

#### `GET /ws`

Stream audio and receive real-time transcript events.

**Query parameters**

| Parameter | Required | Description |
|-----------|----------|-------------|
| `session_id` | no | Resume an existing session. Created automatically if omitted. |
| `tenant_id` | no | Tenant for auto-created sessions |
| `source_type` | no | Default `microphone` |
| `mode` | no | Default `realtime` |

**Sending audio**

Send raw 16 kHz / 16-bit / mono PCM as binary WebSocket frames. Frames should be ~600 ms (9 600 samples = 19 200 bytes). The service buffers and reorders out-of-order frames automatically.

**Receiving events**

The server sends JSON text frames for each transcript update:

```json
{
  "kind": "transcript",
  "transcript_id": "a1b2c3d4-...",
  "text": "[UNKNOWN] Hello there.",
  "start_ms": 0,
  "end_ms": 600,
  "is_final": false,
  "speaker_id": null
}
```

Once speaker identification completes (~2 s after the segment), a second event arrives for the same `transcript_id` with `is_final: true` and `speaker_id` filled in (or still `null` if unrecognised).

The server also sends a WebSocket ping every 15 seconds as a keepalive.

---

## Database Schema

Six migrations run automatically on startup:

| Migration | Tables |
|-----------|--------|
| `001_create_speakers` | `speakers`, `speaker_embeddings` |
| `002_create_sessions` | `audio_sessions` |
| `003_create_segments` | `audio_segments` |
| `004_create_transcripts` | `transcripts` |
| `005_create_logs_and_embeddings` | `inference_logs` |
| `006_create_todos` | `todos` |

`speaker_embeddings.embedding` is stored as a `vector(192)` column (pgvector).

## Development

### Run tests

```bash
cargo test
```

Unit tests run without a database. Integration tests (`tests/test_e2e_session.rs`, `tests/test_repository.rs`) require `DATABASE_URL` or `TEST_DATABASE_URL` to be set.

### Regenerate gRPC stubs

```bash
# Rust stubs are generated automatically by build.rs
cargo build

# Python stubs
cd python-worker
uv run python -m grpc_tools.protoc \
  -I ../proto \
  --python_out=. \
  --grpc_python_out=. \
  ../proto/inference.proto
```

### Lint

```bash
cargo clippy
```
