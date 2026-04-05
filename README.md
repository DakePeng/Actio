# Actio

Actio is a local-first desktop productivity assistant that combines:

- **Real-time speech transcription** (ASR)
- **Speaker-aware session management**
- **Automatic todo generation from transcripts (optional LLM integration)**
- **A Tauri + React desktop reminder board UI**

The project is organized as a monorepo with a Rust backend, a Python gRPC inference worker, and a React/Tauri frontend.

## Repository Layout

```text
Actio/
├── backend/                 # Rust API + websocket server + DB + worker orchestration
│   ├── src/                 # api, engine, repository, domain modules
│   ├── migrations/          # PostgreSQL schema migrations
│   ├── python-worker/       # gRPC worker for ASR/VAD/speaker embedding
│   ├── proto/               # gRPC protobuf definitions
│   └── src-tauri/           # Tauri backend shell
├── frontend/                # React + Vite + Zustand UI
└── LICENSE
```

## Architecture Overview

### 1) Backend (Rust)

The Rust service provides:

- REST APIs for sessions, speakers, transcripts, and todos
- A WebSocket endpoint for receiving binary audio chunks in real time
- PostgreSQL persistence with SQLx migrations
- Python worker process startup and gRPC communication for inference
- Optional LLM-powered todo generation when a session ends

Default API server port: **3000**.

### 2) Inference Worker (Python)

The Python worker exposes gRPC services for:

- VAD (voice activity detection)
- ASR (streaming recognition)
- Speaker embedding extraction

Default worker port: **50051**.

### 3) Frontend (React + Tauri)

The frontend provides:

- A board/tray reminder UX with filtering and quick actions
- Local state management via Zustand
- Mock API fallback for reminders in web mode
- Tauri desktop integration for window mode syncing

Dev web UI port (Vite): **5173** (default Vite behavior).
Mock API port: **3001**.

## API Surface (Current)

The backend currently exposes:

- `GET /health`
- `POST /sessions`
- `GET /sessions/{id}`
- `POST /sessions/{id}/end`
- `GET /sessions/{id}/transcripts`
- `GET /sessions/{id}/todos`
- `POST /speakers`
- `GET /speakers`
- `GET /ws` (WebSocket upgrade)
- `GET /docs` (Swagger UI)

## Prerequisites

- **Rust** (stable toolchain, edition 2024 for backend crate)
- **Python 3.12+** for the worker
- **PostgreSQL 16 + pgvector** (or use Docker Compose from this repo)
- **Node.js + pnpm** for the frontend
- **Tauri prerequisites** (only if running desktop shell)

## Quick Start

### 1) Start PostgreSQL

From `backend/`:

```bash
docker compose up -d postgres
```

This starts a local database at `localhost:5433` with:

- user: `actio`
- password: `actio`
- database: `actio`

### 2) Configure backend environment

Create `backend/.env`:

```env
DATABASE_URL=postgres://actio:actio@localhost:5433/actio
HTTP_PORT=3000
WORKER_HOST=127.0.0.1
WORKER_PORT=50051

# Optional: enables todo generation on session end
# LLM_BASE_URL=https://your-llm-endpoint
# LLM_API_KEY=your-key
# LLM_MODEL=gpt-4o-mini
```

### 3) Start backend

From `backend/`:

```bash
cargo run --bin actio-asr
```

On startup, the backend:

- loads env vars
- runs database migrations
- attempts to start the Python worker
- connects to worker gRPC endpoint
- serves HTTP/WebSocket API

### 4) Start frontend (web dev)

From `frontend/`:

```bash
pnpm install
pnpm dev
```

Optional mock reminder API:

```bash
pnpm mock:api
```

### 5) Run as Tauri desktop app (optional)

From `frontend/`:

```bash
pnpm tauri:dev
```

This delegates into `backend/src-tauri` and launches the desktop shell.

## Development Commands

### Backend

```bash
cd backend
cargo fmt
cargo test
cargo run --bin actio-asr
```

### Python worker (standalone)

```bash
cd backend
python -m venv .venv
source .venv/bin/activate
pip install -r python-worker/requirements.txt
python python-worker/main.py
```

### Frontend

```bash
cd frontend
pnpm install
pnpm dev
pnpm build
pnpm test
```

## Configuration Notes

- `DATABASE_URL` is required.
- `WORKER_HOST`, `WORKER_PORT`, and `HTTP_PORT` have sensible defaults.
- LLM settings are optional; if omitted, todo generation is skipped gracefully.
- Multi-tenant API calls can pass `x-tenant-id` header; if missing, backend uses a nil UUID.

## Current Status

This codebase is in active development and includes foundational pieces for:

- real-time transcription ingestion
- transcript/session persistence
- speaker domain model/repository scaffolding
- optional todo extraction pipeline
- desktop-first productivity UI experience

## License

This project is licensed under the terms in [`LICENSE`](./LICENSE).
