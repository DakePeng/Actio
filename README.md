# Actio

Actio is a local-first desktop productivity assistant that combines:

- **Real-time speech transcription** (ASR, Rust-native via sherpa-onnx)
- **Speaker-aware session management** (diarization + voiceprint identification)
- **Automatic reminder generation from transcripts** (optional LLM integration)
- **A Tauri + React desktop reminder board UI**

The project is a monorepo with a Rust backend (all inference embedded) and a React/Tauri frontend. There is no Python worker — all VAD, ASR, and speaker embedding run in-process via [`sherpa-onnx`](https://github.com/k2-fsa/sherpa-onnx) ONNX Runtime bindings.

## Repository Layout

```text
Actio/
├── backend/                 # Rust API + websocket server + embedded ML + SQLite
│   ├── actio-core/          # Main crate: api, engine, repository, domain
│   ├── src-tauri/           # Tauri desktop shell crate
│   ├── migrations/          # SQLx schema migrations (SQLite)
│   └── Cargo.toml           # Workspace manifest
├── frontend/                # React + Vite + Zustand + Tauri UI
├── spike/                   # Experimental scripts and model reference material
└── LICENSE
```

## Architecture Overview

### Backend (Rust)

A single Axum service provides:

- REST APIs for sessions, speakers, reminders, labels, settings, and models
- A WebSocket endpoint for real-time audio chunks
- Embedded inference:
  - **VAD** — Silero (via `sherpa-onnx::VoiceActivityDetector`)
  - **ASR** — Zipformer (streaming), Whisper, SenseVoice, FunASR Nano, Moonshine — selected per-session
  - **Speaker embedding** — 3D-Speaker (512-dim) via `sherpa-onnx::SpeakerEmbeddingExtractor`
  - **Diarization** — pyannote segmentation + 3D-Speaker clustering
- SQLite persistence via SQLx
- Optional LLM-powered reminder extraction (local llama.cpp or OpenAI-compatible HTTP)

Default HTTP port: **3000**.

### Frontend (React + Tauri)

- Board/tray reminder UX with filtering, swipe, and quick actions
- Zustand stores (`use-store.ts` for reminders, `use-voice-store.ts` for people/voice UI)
- Mock API fallback for reminders in web-only dev mode
- Tauri desktop integration (global shortcuts, window management, native dialogs)

Dev web UI port (Vite): **5173**. Mock reminder API port: **3001**.

## API Surface (Current)

Selected highlights — see `http://localhost:3000/docs` for the full Swagger UI:

- `GET /health`
- `GET /sessions`, `POST /sessions`, `GET /sessions/{id}`, `POST /sessions/{id}/end`
- `GET /sessions/{id}/transcripts`, `GET /sessions/{id}/todos`
- `GET /speakers`, `POST /speakers`, `PATCH /speakers/{id}`, `DELETE /speakers/{id}`
- `POST /speakers/{id}/enroll`
- `GET /reminders`, `POST /reminders`, `POST /reminders/extract`, plus patch/delete by id
- `GET /labels`, `POST /labels`, plus patch/delete by id
- `GET /settings`, `PATCH /settings`, `POST /settings/llm/test`
- `GET/POST /settings/models/*` — model download, warmup, deletion
- `GET /ws` — WebSocket upgrade for audio streaming

## Prerequisites

- **Rust** stable toolchain (edition 2021 for `actio-core`)
- **Node.js + pnpm** for the frontend
- **Tauri prerequisites** (only if running the desktop shell)
- No Python. No Postgres. No Docker.

## Quick Start

### 1. Start the backend

From `backend/`:

```bash
cargo run --bin actio-asr
```

On startup, the backend creates a local SQLite database, runs migrations, loads any already-downloaded models, and serves on `http://localhost:3000`. Download ASR/speaker models via the frontend settings UI or the `/settings/models/download` API.

Optional `.env` in `backend/`:

```env
HTTP_PORT=3000

# Optional: enables reminder extraction on session end via an OpenAI-compatible endpoint
LLM_BASE_URL=https://api.openai.com/v1
LLM_API_KEY=sk-...
LLM_MODEL=gpt-4o-mini
```

Leaving the LLM vars unset disables remote reminder extraction gracefully; the local llama.cpp path is available via the `local-llm` feature (enabled by default) and the `/settings/llm/*` endpoints.

### 2. Start the frontend (web dev)

From `frontend/`:

```bash
pnpm install
pnpm dev
```

Optional mock reminder API (for working on the UI without a running backend):

```bash
pnpm mock:api
```

### 3. Run as a Tauri desktop app

From `frontend/`:

```bash
pnpm tauri:dev
```

This launches the `backend/src-tauri` desktop shell.

## Development Commands

### Backend

```bash
cd backend
cargo fmt
cargo test
cargo run --bin actio-asr
```

### Frontend

```bash
cd frontend
pnpm install
pnpm dev
pnpm build
pnpm test
```

## Current Status

Actively developed. Working:

- Real-time transcription (multiple ASR models, user-selectable)
- Speaker embedding and 1:N identification against the local DB
- Reminder extraction from chat input via LLM (`POST /reminders/extract`)
- Tauri desktop shell with global shortcuts, tray, and dictation
- Model catalog with progressive download and hardware-aware tiering

Not yet implemented / partial:

- Voiceprint enrollment UX in the People tab (backend `POST /speakers/{id}/enroll` is wired; frontend is local-only)
- Cloud ASR fallback
- Multi-tenant auth (`tenant_id` columns exist; no auth layer)

## License

See [`LICENSE`](./LICENSE).
