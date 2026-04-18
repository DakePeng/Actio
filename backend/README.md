# Actio Backend

Rust HTTP/WebSocket service with embedded ML inference. A single Axum process handles REST APIs, WebSocket audio streaming, VAD/ASR/speaker-embedding via [`sherpa-onnx`](https://github.com/k2-fsa/sherpa-onnx), SQLite persistence via SQLx, and optional reminder extraction via a local or remote LLM.

No Python. No gRPC. No separate worker process.

## Architecture

```
Client (audio + REST)
    │  WebSocket /ws + HTTP
    ▼
Axum service (actio-asr)
    ├── engine/vad.rs              — Silero VAD (sherpa-onnx)
    ├── engine/asr.rs              — Streaming + offline ASR (Zipformer, Whisper,
    │                                SenseVoice, FunASR Nano, Moonshine)
    ├── engine/diarization.rs      — pyannote segmentation + 3D-Speaker embeddings
    ├── engine/inference_pipeline  — orchestrates capture → VAD → ASR per session
    ├── engine/model_manager       — on-disk model catalog, download, warmup
    ├── engine/transcript_aggregator — merges partial/final results, backfills speaker tags
    ├── engine/llm_*               — local (llama.cpp) + remote (OpenAI-compatible) LLM
    ├── domain/speaker_matcher     — cosine + Z-Norm 1:N speaker identification
    └── repository/*               — SQLx queries against SQLite
```

**Key design points:**

- Audio: 16 kHz / mono / f32 internally; WebSocket accepts 16-bit PCM and converts
- Speaker embeddings: 3D-Speaker, **512-dim** (stored in SQLite as a stringified vector)
- Speaker identification: cosine similarity + Z-Norm threshold 0.0
- Transcript aggregator emits `[UNKNOWN]` first and backfills speaker IDs once identification completes
- Reminder extraction: optional, runs on session end or ad-hoc via `POST /reminders/extract`

## Prerequisites

- Rust stable (edition 2021)
- No external services — SQLite database file is created next to the binary on first run

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `HTTP_PORT` | no | `3000` | HTTP server port |
| `LLM_BASE_URL` | no | — | OpenAI-compatible endpoint (enables remote LLM) |
| `LLM_API_KEY` | no | — | API key for the remote LLM |
| `LLM_MODEL` | no | `gpt-4o-mini` | Remote model name |

The `local-llm` Cargo feature (enabled by default) bundles `llama-cpp-2` for on-device GGUF inference; toggle it off with `--no-default-features` if you want a smaller binary.

Create a `backend/.env` if you want to use a remote LLM:

```env
LLM_BASE_URL=https://api.openai.com/v1
LLM_API_KEY=sk-...
```

## Running

```bash
cargo run --bin actio-asr
```

On startup the service:

1. Loads `.env` via `dotenvy`
2. Opens the SQLite database and runs pending migrations
3. Scans the model directory for previously-downloaded model packs
4. Serves HTTP on `0.0.0.0:3000`

Models are fetched on demand via `POST /settings/models/download`; the frontend settings UI drives this.

## API Endpoints

Interactive docs: `http://localhost:3000/docs` (Swagger UI). Raw OpenAPI: `GET /api-docs/openapi.json`.

---

### Health

#### `GET /health`

```json
{
  "active_sessions": 2,
  "uptime_secs": 3600,
  "worker_state": "embedded",
  "local_route_count": 142,
  "worker_error_count": 0,
  "unknown_speaker_count": 5
}
```

`worker_state` is always `"embedded"` — there is no separate worker; the field is preserved for API backward compatibility.

---

### Sessions

- `POST /sessions` — start a session (body optional; defaults `source_type=microphone`, `mode=realtime`)
- `GET /sessions` — list sessions
- `GET /sessions/{id}` — session details
- `POST /sessions/{id}/end` — idempotent; triggers LLM reminder generation if configured
- `GET /sessions/{id}/transcripts` — ordered transcripts for the session
- `GET /sessions/{id}/todos` — reminders extracted from the session (legacy route name)

---

### Speakers

- `POST /speakers` — register a speaker (metadata only; use `/enroll` to add a voiceprint)
- `GET /speakers` — list speakers for the tenant
- `PATCH /speakers/{id}` — update display name or color
- `DELETE /speakers/{id}` — cascade-deletes embeddings; historical segment attributions become NULL
- `POST /speakers/{id}/enroll` — upload audio samples to extract and store 512-dim embeddings

All speaker routes accept optional `x-tenant-id: <uuid>` (falls back to nil UUID).

---

### Reminders & Labels

- `GET /reminders`, `POST /reminders`, `POST /reminders/extract`
- `GET /reminders/{id}`, `PATCH /reminders/{id}`, `DELETE /reminders/{id}`
- `GET /labels`, `POST /labels`, `PATCH /labels/{id}`, `DELETE /labels/{id}`

---

### WebSocket

#### `GET /ws`

Stream audio and receive transcript events.

**Query parameters**: `session_id` (optional, created automatically if omitted), `tenant_id`, `source_type`, `mode`.

**Sending audio**: raw 16 kHz / 16-bit / mono PCM as binary frames.

**Receiving events**: JSON text frames per transcript update:

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

A second event for the same `transcript_id` arrives after speaker identification completes (~2 s) with `speaker_id` filled in (or still `null` if unrecognised).

---

### Settings & Models

- `GET /settings`, `PATCH /settings`
- `POST /settings/llm/test` — verifies an LLM endpoint
- `GET /settings/models`, `GET /settings/models/available`
- `POST /settings/models/download`, `POST /settings/models/cancel-download`
- `POST /settings/models/warmup`, `DELETE /settings/models/{id}`
- `GET /settings/audio-devices`
- `GET /settings/llm/models`, `DELETE /settings/llm/models/{id}`
- `POST /settings/llm/load`, `POST /settings/llm/cancel-load`, `GET /settings/llm/load-status`
- OpenAI-compatible shim: `GET /v1/models`, `POST /v1/chat/completions`

---

## Database Schema

Migrations run automatically on startup against a local SQLite file. Current migrations:

| Migration | Purpose |
|-----------|---------|
| `001_create_speakers` | `speakers` |
| `002_create_sessions` | `audio_sessions` |
| `003_create_segments` | `audio_segments` |
| `004_create_transcripts` | `transcripts` |
| `005_create_logs_and_embeddings` | `speaker_embeddings`, `verification_logs`, `routing_decision_logs` |
| `006_create_todos` | `todos` (later renamed) |
| `007_rename_todos_to_reminders` | rename + schema adjustments |
| `008_create_labels` | `labels` + reminder link |
| `009_create_sessions_index` | query indexes |
| `010_reminders_session_nullable` | manual reminder support |

> **Known stale content:** `005_create_logs_and_embeddings.sql` references `CREATE EXTENSION "vector"` and `vector(192)` which assumed Postgres + pgvector + CAM++. The real storage is SQLite with a stringified vector column, and the actual embedding dimension is 512 (3D-Speaker). A follow-up migration will reconcile this when voiceprint enrollment lands in the frontend.

## Development

```bash
cargo test          # unit + integration tests (no external services required)
cargo fmt
cargo clippy
```
