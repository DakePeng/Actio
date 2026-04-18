# Actio Backend — Architecture & Design

> This document replaces an earlier design (2026-03) that assumed a Rust-controlled Python gRPC worker running FunASR and CAM++. That architecture was never implemented. The actual backend is a single Rust process with `sherpa-onnx`-embedded inference. This document describes what is actually built.

## 1. Goals

1. **Real-time speech transcription** — partial + final transcripts over WebSocket, <500 ms partial latency on mid-range hardware.
2. **Speaker-aware sessions** — identify registered speakers in real time; tag unknowns with `[UNKNOWN]` and allow retroactive enrollment.
3. **Automatic reminder extraction** — convert finished session transcripts into actionable reminders via an LLM.
4. **Local-first, offline-capable** — everything runs on the user's machine by default. Remote LLMs are optional.
5. **Single-user desktop productivity** — no multi-tenant auth, no cloud orchestration. `tenant_id` is present in the schema for a possible future but defaults to the nil UUID.

Non-goals: distributed deployment, cloud-only operation, custom model training, multi-region failover, billing.

## 2. High-Level Architecture

```
[React + Tauri Frontend]
       │  HTTP / WebSocket
       ▼
[Axum Service: actio-asr]  — single Rust process
 ├─ api/            Route handlers (sessions, speakers, reminders, labels, settings, models, ws)
 ├─ engine/
 │   ├─ audio_capture      cpal input device → f32 PCM stream
 │   ├─ vad                Silero VAD (sherpa-onnx)
 │   ├─ asr                Zipformer / Whisper / SenseVoice / FunASR Nano / Moonshine
 │   ├─ diarization        pyannote segmentation + 3D-Speaker embeddings (for offline / post-hoc)
 │   ├─ inference_pipeline Per-session VAD→ASR wiring, cancellable
 │   ├─ model_manager      Catalog, download, warmup, hardware-aware tiering
 │   ├─ transcript_aggregator Merges partials/finals, backfills speaker IDs
 │   ├─ llm_*              Local (llama.cpp) + remote (OpenAI-compat) reminder extraction
 │   └─ metrics            Counters surfaced via /health
 ├─ domain/
 │   ├─ speaker_matcher    Cosine + Z-Norm 1:N identification
 │   └─ types              Shared serde/utoipa types
 └─ repository/            SQLx queries against SQLite
```

Inference libraries are Rust-native:

- `sherpa-onnx` (ONNX Runtime wrapper) — VAD, ASR, diarization, speaker embedding
- `llama-cpp-2` (optional, feature-gated) — local GGUF LLM inference
- `reqwest` — remote LLM via OpenAI-compatible HTTP

No subprocess. No gRPC. No separate runtime.

## 3. Data Model

SQLite, managed by `sqlx::migrate`. All tables include a `tenant_id` column defaulting to the nil UUID.

- **speakers** — `id`, `tenant_id`, `display_name`, `status`, `created_at` (+ `color` to be added when enrollment lands in the UI)
- **speaker_embeddings** — `id`, `speaker_id`, `embedding` (512-dim vector, serialized), `duration_ms`, `quality_score`, `is_primary`, `embedding_dimension`, `created_at`. Cascade-deleted with the parent speaker.
- **audio_sessions** — `id`, `tenant_id`, `source_type`, `mode`, `started_at`, `ended_at`, `metadata`
- **audio_segments** — `id`, `session_id`, `start_ms`, `end_ms`, `speaker_id` (nullable; SET NULL on speaker delete), audio reference metadata
- **transcripts** — `id`, `session_id`, `segment_id`, `start_ms`, `end_ms`, `text`, `is_final`, `backend_type`, `created_at`
- **reminders** (formerly `todos`) — `id`, `session_id` (nullable for manual reminders), `assigned_to`, `description`, `status`, `priority`, `created_at`, `updated_at`
- **labels** — `id`, `name`, color, plus a link table to reminders
- **verification_logs**, **routing_decision_logs** — audit trails for speaker match decisions and (future) routing decisions

The earlier migration `005_create_logs_and_embeddings.sql` was written against Postgres + pgvector and declares `vector(192)` for CAM++; the SQLite engine stores embeddings as a stringified vector and the actual dimension is 512. A reconciling migration is planned as part of the enrollment work.

## 4. Real-time Transcription Flow

1. Client opens `GET /ws?session_id=<id>` (created implicitly if omitted).
2. Backend creates an `InferencePipeline`:
   - `audio_capture::start_capture` feeds a `tokio::mpsc` of `Vec<f32>` at 16 kHz.
   - Depending on the selected ASR model, audio goes either directly into a streaming recognizer (Zipformer) or through a VAD that emits `SpeechSegment`s into an offline recognizer (Whisper, SenseVoice, etc.).
   - Each recognizer runs inside `tokio::task::spawn_blocking` because `sherpa_onnx::OnlineRecognizer` / `VoiceActivityDetector` hold raw pointers and are `!Send`. A `crossbeam_channel` bridges between the blocking thread and tokio.
3. The `TranscriptAggregator` receives partial and final results, writes rows to `transcripts`, and pushes JSON frames to the WebSocket. Speaker tags start as `[UNKNOWN]` and are backfilled after identification finishes.
4. On segment completion, audio for the segment is passed to `sherpa_onnx::SpeakerEmbeddingExtractor`; the resulting 512-dim embedding is fed to `domain::speaker_matcher::identify_speaker`, which performs a cosine-similarity scan across the tenant's embeddings, Z-Norm normalises, and picks the best match above threshold 0. The `speaker_id` is written back on the transcript row and a second WebSocket frame is emitted.
5. On `POST /sessions/{id}/end`, if an LLM endpoint is configured, a background task extracts reminders from the full transcript.

## 5. Speaker Enrollment (current + planned)

Backend routes are already wired:

- `POST /speakers` — create (metadata only)
- `PATCH /speakers/{id}` — edit display name / color
- `DELETE /speakers/{id}` — cascade
- `POST /speakers/{id}/enroll` — accept one or more audio clips, extract embeddings, persist them

Planned (tracked in the forthcoming feature spec):

1. Frontend People tab switches from `useVoiceStore` (local-only) to calling the real `/speakers` API.
2. "Record now" enrollment captures 2–3 short clips, extracts 512-dim embeddings, and stores them (first marked `is_primary = true`).
3. Retroactive tagging: during or after a session, the user assigns a `[UNKNOWN]` segment to an existing or new speaker; the segment's embedding becomes that speaker's first voiceprint.
4. Re-enrollment replaces all embeddings for a speaker.

## 6. Reminder Extraction

Two paths, both optional:

- **Remote**: OpenAI-compatible HTTP endpoint (`LLM_BASE_URL` / `LLM_API_KEY` / `LLM_MODEL`).
- **Local**: llama.cpp (`local-llm` feature, default-on). The frontend manages GGUF model download and loading via `/settings/llm/*`.

Triggers:

- `POST /sessions/{id}/end` — background task with ~90 s timeout; idempotent
- `POST /reminders/extract` — ad-hoc from free text (e.g., the chat composer)

The OpenAI-compatible shim (`GET /v1/models`, `POST /v1/chat/completions`) lets the local llama.cpp instance be used by any OpenAI SDK-based client.

## 7. Observability

- **`/health`** — surface counters: active sessions, uptime, local route count, worker error count, unknown speaker count. `worker_state` is always `"embedded"`.
- **Structured logging** via `tracing` with JSON output support.
- **Metrics** tracked in `engine::metrics` via atomics.

## 8. Error Handling & Failure Modes

| Failure | Response |
|---------|----------|
| Model not downloaded | `InferencePipeline::start_session` returns an error; API surfaces a clear status. |
| Audio device missing | Session start fails with a diagnostic; frontend prompts device selection. |
| Recognizer panic / OOM | `spawn_blocking` isolates the crash; the session ends with an error. No restart loop — the user re-starts the session. |
| LLM endpoint unreachable | Reminder extraction logs an error; existing transcripts are unaffected. |
| Client disconnects mid-stream | Pipeline is cancelled via oneshot; partial transcripts are flushed. |
| Speaker has no embeddings yet | Identification returns `None`; transcripts stay `[UNKNOWN]`. |

There is no circuit breaker (no remote inference dependency to break). The earlier design's Closed/Open/HalfOpen state machine was tied to the removed Python worker.

## 9. Extensibility

- **Adding an ASR model** — extend `model_manager` catalog + add a branch in `inference_pipeline::start_session`. New entries can ship without touching the API layer.
- **Swapping the speaker embedding model** — update `SpeakerEmbeddingExtractor` instantiation in `engine::diarization`. Note `embedding_dimension` on existing rows; old embeddings become invalid if the dimension changes.
- **Cloud ASR fallback** — would slot into `engine::inference_pipeline` alongside the local recognizer, selected by a routing policy. Not currently implemented.

## 10. Known Gaps

- No auth / multi-tenant isolation (tenant_id plumbing exists but is unused).
- No frontend voiceprint enrollment UX yet.
- Migration 005 uses pgvector syntax inappropriate for SQLite; embeddings work because the repository layer stores a stringified vector, but the schema definition is misleading.
- The earlier Chinese design doc contained valuable architectural thinking (routing policies, privacy edges, circuit breaker, Z-Norm) that remains aspirational. Where those ideas are still useful they have been folded into the sections above.
