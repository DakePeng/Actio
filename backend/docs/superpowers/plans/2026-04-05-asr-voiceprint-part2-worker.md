# ASR + 声纹 Recognition System — MVP Implementation Plan (Part 2: Worker & gRPC)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Build the Rust ↔ Python Worker communication layer, including worker process lifecycle management, gRPC client, inference router, and Python services for VAD, ASR, and Speaker embedding.

**Prerequisites:** Part 1 (foundation) completed — Cargo.toml, proto definitions, Python worker skeleton, database layer.

---

## Task 7: Worker Process Manager

**Files:**
- Create: `src/engine/mod.rs`
- Create: `src/engine/worker.rs`

**Purpose:** Rust manages the Python Worker as a subprocess — starts it, monitors health, restarts on failure (max 3 restarts), and shuts it down cleanly.

- [x] **Step 1: Create engine module exports**

```rust
// src/engine/mod.rs
pub mod worker;
```

- [x] **Step 2: Write worker manager**

```rust
// src/engine/worker.rs
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};

pub struct WorkerManager {
    port: u16,
    process: Arc<Mutex<Option<Child>>>,
    restart_count: Arc<Mutex<u32>>,
}

impl WorkerManager {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            process: Arc::new(Mutex::new(None)),
            restart_count: Arc::new(Mutex::new(0)),
        }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        self.spawn_worker().await?;
        self.spawn_monitor().await;
        Ok(())
    }

    async fn spawn_worker(&self) -> anyhow::Result<()> {
        let mut proc_lock = self.process.lock().await;
        if proc_lock.is_some() {
            return Ok(());
        }

        info!(port = self.port, "Starting Python Worker");

        let child = Command::new("python")
            .arg("python-worker/main.py")
            .env("WORKER_PORT", self.port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        *proc_lock = Some(child);
        drop(proc_lock);

        // Wait for worker to be ready (health check)
        sleep(Duration::from_secs(2)).await;

        info!("Python Worker started successfully");
        Ok(())
    }

    async fn spawn_monitor(&self) {
        let process = self.process.clone();
        let restart_count = self.restart_count.clone();
        let port = self.port;

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(3)).await;

                let mut proc = process.lock().await;
                if let Some(ref mut child) = *proc {
                    match child.try_wait() {
                        Ok(None) => {
                            // Still running — health check would go here
                        }
                        Ok(Some(status)) => {
                            warn!(exit_code = ?status.code(), "Python Worker exited");
                            *proc = None;

                            let mut count = restart_count.lock().await;
                            if *count < 3 {
                                *count += 1;
                                warn!(attempt = *count, "Restarting Python Worker");
                                drop(proc);
                                drop(count);

                                sleep(Duration::from_secs(1)).await;
                                // Spawn new worker
                                if let Ok(new_child) = Command::new("python")
                                    .arg("python-worker/main.py")
                                    .env("WORKER_PORT", port.to_string())
                                    .stdout(Stdio::piped())
                                    .stderr(Stdio::piped())
                                    .spawn()
                                {
                                    *process.lock().await = Some(new_child);
                                }
                            } else {
                                error!("Max restarts reached; worker in failed state");
                            }
                        }
                        Err(e) => {
                            error!("Failed to check worker status: {}", e);
                        }
                    }
                }
            }
        });
    }

    pub async fn stop(&self) {
        let mut proc = self.process.lock().await;
        if let Some(mut child) = proc.take() {
            let _ = child.kill().await;
            info!("Python Worker stopped");
        }
    }
}
```

- [x] **Step 3: Verify it compiles**

```bash
cargo check
```

Expected: Compiles (may warn about unused code).

- [x] **Step 4: Commit**

```bash
git add src/engine/worker.rs src/engine/mod.rs
git commit -m "feat: Python worker process lifecycle manager"
```

---

## Task 8: gRPC Client + Inference Router

**Files:**
- Create: `src/engine/grpc_client.rs`
- Create: `src/engine/inference_router.rs`

This task creates the Rust-side gRPC client that communicates with the Python Worker, and the inference router that decides when to use local vs. cloud.

- [x] **Step 1: Write gRPC client wrapper**

```rust
// src/engine/grpc_client.rs
use tonic::transport::Channel;
use tracing::{info, warn, error};

use crate::grpc::inference::*;
use crate::error::AppError;

#[derive(Clone)]
pub struct GrpcClient {
    channel: Channel,
}

impl GrpcClient {
    pub async fn connect(addr: &str) -> Result<Self, tonic::transport::Error> {
        let channel = Channel::from_shared(addr.to_string())?.connect().await?;
        info!(addr, "gRPC client connected");
        Ok(Self { channel })
    }

    // --- VAD ---
    pub async fn detect_speech(
        &self,
        chunks: Vec<(Vec<u8>, i64, i32, String)>,
    ) -> Result<Vec<VADResult>, AppError> {
        let mut client = VadServiceClient::new(self.channel.clone());
        let (tx, rx) = tokio::sync::mpsc::channel(chunks.len());

        tokio::spawn(async move {
            for (data, ts, seq, session_id) in chunks {
                let _ = tx.send(AudioChunk {
                    audio_data: data,
                    timestamp_ms: ts,
                    sequence_num: seq,
                    session_id,
                }).await;
            }
        });

        let response = client.detect_speech(tokio_stream::wrappers::ReceiverStream::new(rx)).await?;
        let mut results = vec![];

        let mut stream = response.into_inner();
        while let Some(result) = stream.message().await? {
            results.push(result);
        }

        Ok(results)
    }

    // --- ASR Streaming ---
    pub async fn stream_recognize(
        &self,
        chunks: Vec<(Vec<u8>, i64, i32, String)>,
    ) -> Result<Vec<RecognizeResult>, AppError> {
        let mut client = AsrServiceClient::new(self.channel.clone());
        let (tx, rx) = tokio::sync::mpsc::channel(chunks.len());

        tokio::spawn(async move {
            for (data, ts, seq, session_id) in chunks {
                let _ = tx.send(AudioChunk {
                    audio_data: data,
                    timestamp_ms: ts,
                    sequence_num: seq,
                    session_id,
                }).await;
            }
        });

        let response = client.stream_recognize(tokio_stream::wrappers::ReceiverStream::new(rx)).await?;
        let mut results = vec![];

        let mut stream = response.into_inner();
        while let Some(result) = stream.message().await? {
            results.push(result);
        }

        Ok(results)
    }

    // --- Speaker Embedding ---
    pub async fn extract_embedding(
        &self,
        audio_data: Vec<u8>,
        sample_rate: f32,
    ) -> Result<EmbeddingResponse, AppError> {
        let mut client = SpeakerServiceClient::new(self.channel.clone());

        let response = client.extract_embedding(ExtractEmbeddingRequest {
            audio_data,
            sample_rate,
        }).await?;

        Ok(response.into_inner())
    }

    // --- Speaker Verify ---
    pub async fn verify_speaker(
        &self,
        audio_data: Vec<u8>,
        reference_embedding: Vec<f32>,
    ) -> Result<VerifySpeakerResponse, AppError> {
        let mut client = SpeakerServiceClient::new(self.channel.clone());

        let response = client.verify_speaker(VerifySpeakerRequest {
            audio_data,
            reference_embedding,
        }).await?;

        Ok(response.into_inner())
    }

    // --- Health Check ---
    pub async fn health_check(&self) -> Result<HealthCheckResponse, AppError> {
        let mut client = SpeakerServiceClient::new(self.channel.clone());
        let response = client.health_check(HealthCheckRequest {}).await?;
        Ok(response.into_inner())
    }
}
```

- [x] **Step 2: Write inference router with circuit breaker integration**

```rust
// src/engine/inference_router.rs
use crate::engine::grpc_client::GrpcClient;
use crate::error::AppError;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};
use crate::grpc::inference::*;

// Stub circuit breaker for Part 2 (full implementation in Part 3)
pub struct CircuitBreaker;
impl CircuitBreaker {
    pub fn new() -> Self { Self }
    pub fn allow_local(&mut self) -> bool { true }
    pub fn record_success(&mut self) {}
    pub fn record_failure(&mut self) {}
}

pub struct InferenceRouter {
    grpc_client: GrpcClient,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    cloud_available: bool,
}

impl InferenceRouter {
    pub fn new(grpc_client: GrpcClient, circuit_breaker: Arc<Mutex<CircuitBreaker>>) -> Self {
        Self {
            grpc_client,
            circuit_breaker,
            cloud_available: false, // MVP: no cloud
        }
    }

    /// Route ASR request through the pipeline.
    /// MVP: always try local first, no cloud fallback.
    pub async fn route_asr(
        &self,
        chunks: Vec<(Vec<u8>, i64, i32, String)>,
    ) -> Result<Vec<RecognizeResult>, AppError> {
        let mut cb = self.circuit_breaker.lock().await;
        if !cb.allow_local() {
            warn!("Circuit breaker OPEN, local ASR unavailable");
            return Err(AppError::WorkerUnavailable("Circuit breaker open".into()));
        }
        drop(cb);

        match self.grpc_client.stream_recognize(chunks).await {
            Ok(results) => {
                self.circuit_breaker.lock().await.record_success();
                Ok(results)
            }
            Err(e) => {
                self.circuit_breaker.lock().await.record_failure();
                Err(e)
            }
        }
    }

    /// Route speaker embedding extraction.
    pub async fn route_extract_embedding(
        &self,
        audio_data: Vec<u8>,
        sample_rate: f32,
    ) -> Result<EmbeddingResponse, AppError> {
        self.grpc_client.extract_embedding(audio_data, sample_rate).await
    }

    /// Route speaker verification.
    pub async fn route_verify_speaker(
        &self,
        audio_data: Vec<u8>,
        reference_embedding: Vec<f32>,
    ) -> Result<VerifySpeakerResponse, AppError> {
        self.grpc_client.verify_speaker(audio_data, reference_embedding).await
    }
}
```

- [x] **Step 3: Update engine/mod.rs to export new modules**

```rust
// src/engine/mod.rs
pub mod worker;
pub mod grpc_client;
pub mod inference_router;
```

- [x] **Step 4: Verify it compiles**

```bash
cargo check
```

Expected: Compiles.

- [x] **Step 5: Commit**

```bash
git add src/engine/grpc_client.rs src/engine/inference_router.rs
git commit -m "feat: gRPC client wrapper and inference router"
```

---

## Task 9: REST + WebSocket API Endpoints

**Files:**
- Create: `src/api/mod.rs`
- Create: `src/api/session.rs`

This task creates the REST endpoints for session and speaker management, plus the WebSocket handler skeleton for audio streaming.

- [x] **Step 1: Create REST session endpoints**

```rust
// src/api/session.rs
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::repository::session;
use crate::repository::speaker;
use crate::repository::transcript;
use crate::domain::types::AudioSession;
use crate::domain::types::Speaker;

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub source_type: Option<String>,
    pub mode: Option<String>,
}

#[derive(Serialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub started_at: String,
}

pub async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), AppApiError> {
    let tenant_id = Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    let s = session::create_session(&state.pool, tenant_id).await
        .map_err(|e| AppApiError(e.to_string()))?;

    Ok((StatusCode::CREATED, Json(SessionResponse {
        id: s.id,
        started_at: s.started_at.to_rfc3339(),
    })))
}

pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AudioSession>, AppApiError> {
    let s = session::get_session(&state.pool, id).await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(s))
}

pub async fn end_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    session::end_session(&state.pool, id).await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Speaker endpoints ---

#[derive(Deserialize)]
pub struct CreateSpeakerRequest {
    pub display_name: String,
}

pub async fn create_speaker(
    State(state): State<AppState>,
    Json(req): Json<CreateSpeakerRequest>,
) -> Result<(StatusCode, Json<Speaker>), AppApiError> {
    let tenant_id = Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    let s = speaker::create_speaker(&state.pool, &req.display_name, tenant_id).await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(s)))
}

pub async fn list_speakers(
    State(state): State<AppState>,
) -> Result<Json<Vec<Speaker>>, AppApiError> {
    let speakers = speaker::list_speakers(&state.pool).await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(speakers))
}

// --- Transcript endpoints ---

pub async fn get_transcripts(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<crate::domain::types::Transcript>>, AppApiError> {
    let transcripts = transcript::get_transcripts_for_session(&state.pool, id).await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(transcripts))
}

// --- Error type ---

#[derive(Debug)]
pub struct AppApiError(String);

impl axum::response::IntoResponse for AppApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0).into_response()
    }
}
```

- [x] **Step 2: Create API module with router**

```rust
// src/api/mod.rs
use axum::{
    routing::{get, post},
    Router,
};
mod session;

pub use session::AppApiError;

use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", post(session::create_session))
        .route("/sessions/{id}", get(session::get_session))
        .route("/sessions/{id}/end", post(session::end_session))
        .route("/sessions/{id}/transcripts", get(session::get_transcripts))
        .route("/speakers", post(session::create_speaker))
        .route("/speakers", get(session::list_speakers))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
```

- [x] **Step 3: Verify it compiles**

```bash
cargo check
```

- [x] **Step 4: Commit**

```bash
git add src/api/mod.rs src/api/session.rs
git commit -m "feat: REST API endpoints for sessions, speakers, and transcripts"
```

---

## Task 10: Python Worker Services (VAD, ASR, Speaker) + Model Loader

**Files:**
- Create: `python-worker/models/__init__.py`
- Create: `python-worker/models/loader.py`
- Create: `python-worker/services/vad.py`
- Create: `python-worker/services/asr.py`
- Create: `python-worker/services/speaker.py`
- Modify: `python-worker/main.py` (register services)
- Modify: `python-worker/services/__init__.py`

This task implements the actual Python gRPC service handlers with model loading.

- [x] **Step 1: Create model loader**

```python
# python-worker/models/__init__.py
from .loader import ModelLoader
```

```python
# python-worker/models/loader.py
"""Model loading and lifecycle management for FunASR and CAM++."""
import logging
import threading
from dataclasses import dataclass
from typing import Optional

logger = logging.getLogger("actio-worker.models")


@dataclass
class ModelState:
    vad_ready: bool = False
    asr_ready: bool = False
    speaker_ready: bool = False
    error_detail: str = ""


class ModelLoader:
    """Manages loading and caching of inference models.

    Models are loaded lazily on first request and cached for the lifetime of the process.
    """

    def __init__(self):
        self._lock = threading.Lock()
        self._state = ModelState()
        self._asr_model = None
        self._speaker_model = None
        self._vad_model = None

    @property
    def state(self) -> ModelState:
        return self._state

    def load_asr_model(self):
        """Load FunASR Paraformer-Streaming model for streaming ASR."""
        with self._lock:
            if self._asr_model is not None:
                return self._asr_model
            try:
                from funasr import AutoModel
                logger.info("Loading FunASR Paraformer-Streaming model...")
                self._asr_model = AutoModel(
                    model="paraformer-zh-streaming",
                    model_revision="v2.0.4",
                    disable_update=True,
                )
                self._state.asr_ready = True
                logger.info("FunASR model loaded successfully")
            except Exception as e:
                self._state.error_detail = f"ASR model load failed: {e}"
                logger.error(f"Failed to load ASR model: {e}")
                raise

    def load_speaker_model(self):
        """Load CAM++ speaker embedding model from 3D-Speaker."""
        with self._lock:
            if self._speaker_model is not None:
                return self._speaker_model
            try:
                from modelscope.pipelines import pipeline
                logger.info("Loading CAM++ speaker embedding model...")
                self._speaker_model = pipeline(
                    task="speaker-verification",
                    model="iic/speech_campplus_sv_zh-cn_16k-common",
                )
                self._state.speaker_ready = True
                logger.info("CAM++ model loaded successfully")
            except Exception as e:
                self._state.error_detail = f"Speaker model load failed: {e}"
                logger.error(f"Failed to load speaker model: {e}")
                raise

    def get_asr_model(self):
        if self._asr_model is None:
            self.load_asr_model()
        return self._asr_model

    def get_speaker_model(self):
        if self._speaker_model is None:
            self.load_speaker_model()
        return self._speaker_model
```

- [x] **Step 2: Create VAD gRPC service**

```python
# python-worker/services/vad.py
"""VAD gRPC service — detects speech segments in audio chunks."""
import logging
import grpc

import inference_pb2
import inference_pb2_grpc

logger = logging.getLogger("actio-worker.vad")


class VADService(inference_pb2_grpc.VADServiceServicer):
    """VAD service that detects speech in audio streams.

    MVP implementation: energy-based VAD (simple RMS threshold).
    Future: integrate Silero VAD or FunASR VAD model.
    """

    SPEECH_THRESHOLD = 0.01  # RMS threshold for speech detection
    MIN_SEGMENT_MS = 300     # Minimum segment duration

    def DetectSpeech(self, request_iterator, context):
        """Process streaming audio chunks and return VAD results."""
        buffer = bytearray()
        segment_start_ms = None
        session_id = ""

        for chunk in request_iterator:
            audio_data = chunk.audio_data
            session_id = chunk.session_id
            timestamp_ms = chunk.timestamp_ms

            # Simple energy-based VAD
            is_speech = self._is_speech(audio_data)

            if is_speech and segment_start_ms is None:
                segment_start_ms = timestamp_ms
            elif not is_speech and segment_start_ms is not None:
                duration = timestamp_ms - segment_start_ms
                if duration >= self.MIN_SEGMENT_MS:
                    yield inference_pb2.VADResult(
                        is_speech=True,
                        segment_start_ms=segment_start_ms,
                        segment_end_ms=timestamp_ms,
                        confidence=0.8,
                        session_id=session_id,
                    )
                segment_start_ms = None

        # Flush final segment if still active
        if segment_start_ms is not None:
            yield inference_pb2.VADResult(
                is_speech=True,
                segment_start_ms=segment_start_ms,
                segment_end_ms=segment_start_ms + self.MIN_SEGMENT_MS,
                confidence=0.7,
                session_id=session_id,
            )

    @staticmethod
    def _is_speech(audio_data: bytes) -> bool:
        """Simple energy-based VAD using RMS of 16-bit PCM samples."""
        import struct
        if len(audio_data) < 2:
            return False

        # Unpack 16-bit signed PCM samples
        num_samples = len(audio_data) // 2
        samples = struct.unpack(f'<{num_samples}h', audio_data[:num_samples * 2])

        if not samples:
            return False

        # RMS energy
        rms = (sum(s * s for s in samples) / len(samples)) ** 0.5
        # Normalize to [-1, 1] range (16-bit max = 32768)
        normalized = rms / 32768.0

        return normalized > 0.01
```

- [x] **Step 3: Create ASR gRPC service**

```python
# python-worker/services/asr.py
"""ASR gRPC service — streaming speech recognition using FunASR."""
import logging
import grpc

import inference_pb2
import inference_pb2_grpc

logger = logging.getLogger("actio-worker.asr")


class ASRService(inference_pb2_grpc.ASRServiceServicer):
    """ASR service using FunASR Paraformer-Streaming for real-time recognition."""

    def __init__(self, model_loader):
        self._model_loader = model_loader

    def StreamRecognize(self, request_iterator, context):
        """Bidirectional streaming ASR — receive audio chunks, return transcripts."""
        try:
            model = self._model_loader.get_asr_model()
        except Exception as e:
            logger.error(f"Cannot load ASR model: {e}")
            context.set_code(grpc.StatusCode.UNAVAILABLE)
            context.set_details(f"ASR model not available: {e}")
            return

        import numpy as np

        chunk_buffer = bytearray()
        chunk_count = 0
        session_id = ""

        for chunk in request_iterator:
            chunk_buffer.extend(chunk.audio_data)
            chunk_count += 1
            session_id = chunk.session_id

            # Process every ~2 chunks (~1.2s of audio) for partial results
            if chunk_count % 2 == 0 and len(chunk_buffer) > 0:
                try:
                    # Convert bytes to numpy array (16-bit PCM -> float32)
                    audio_np = np.frombuffer(bytes(chunk_buffer), dtype=np.int16).astype(np.float32) / 32768.0

                    result = model.generate(
                        input=audio_np,
                        batch_size_s=300,
                    )

                    if result and result[0].get("text"):
                        text = result[0]["text"]
                        is_final = chunk_count % 10 == 0  # Periodic final

                        yield inference_pb2.RecognizeResult(
                            text=text,
                            is_final=is_final,
                            start_ms=0,
                            end_ms=chunk_count * 600,
                            session_id=session_id,
                        )

                        if is_final:
                            chunk_buffer.clear()

                except Exception as e:
                    logger.error(f"ASR inference error: {e}")

    def RecognizeFile(self, request, context):
        """Unary file recognition — process entire audio at once."""
        try:
            model = self._model_loader.get_asr_model()
        except Exception as e:
            context.set_code(grpc.StatusCode.UNAVAILABLE)
            context.set_details(f"ASR model not available: {e}")
            return inference_pb2.RecognizeFileResponse()

        import numpy as np

        audio_np = np.frombuffer(request.audio_data, dtype=np.int16).astype(np.float32) / 32768.0

        try:
            result = model.generate(input=audio_np, batch_size_s=300)
            segments = []
            if result and result[0].get("text"):
                segments.append(inference_pb2.TranscriptSegment(
                    text=result[0]["text"],
                    start_ms=0,
                    end_ms=int(len(audio_np) / 16),  # Approximate
                    is_final=True,
                ))

            return inference_pb2.RecognizeFileResponse(segments=segments)

        except Exception as e:
            logger.error(f"File recognition error: {e}")
            return inference_pb2.RecognizeFileResponse()
```

- [x] **Step 4: Create Speaker gRPC service**

```python
# python-worker/services/speaker.py
"""Speaker gRPC service — embedding extraction and verification using CAM++."""
import logging
import grpc

import inference_pb2
import inference_pb2_grpc

logger = logging.getLogger("actio-worker.speaker")


class SpeakerService(inference_pb2_grpc.SpeakerServiceServicer):
    """Speaker service using CAM++ for 192-dim embedding extraction."""

    def __init__(self, model_loader):
        self._model_loader = model_loader

    def ExtractEmbedding(self, request, context):
        """Extract 192-dim speaker embedding from audio using CAM++."""
        try:
            model = self._model_loader.get_speaker_model()
        except Exception as e:
            logger.error(f"Cannot load speaker model: {e}")
            context.set_code(grpc.StatusCode.UNAVAILABLE)
            context.set_details(f"Speaker model not available: {e}")
            return inference_pb2.EmbeddingResponse()

        import numpy as np

        audio_data = request.audio_data
        sample_rate = int(request.sample_rate) if request.sample_rate > 0 else 16000

        # Convert to numpy float32
        audio_np = np.frombuffer(audio_data, dtype=np.int16).astype(np.float32) / 32768.0

        duration_ms = len(audio_np) / sample_rate * 1000

        # CAM++ needs at least 2s of audio for stable embeddings
        if duration_ms < 2000:
            logger.warning(f"Audio too short for embedding: {duration_ms}ms")
            return inference_pb2.EmbeddingResponse(
                embedding=[],
                quality_score=0.0,
                duration_ms=duration_ms,
            )

        try:
            # Use CAM++ to extract embedding
            result = model(audio_np)

            # Extract embedding vector
            if isinstance(result, dict) and "embedding" in result:
                embedding = result["embedding"].flatten().tolist()
            elif hasattr(result, 'embedding'):
                embedding = np.array(result.embedding).flatten().tolist()
            else:
                # Fallback: try to extract from the raw output
                embedding = np.array(result).flatten().tolist()

            logger.info(f"Extracted {len(embedding)}-dim embedding from {duration_ms:.0f}ms audio")

            return inference_pb2.EmbeddingResponse(
                embedding=embedding[:192],  # CAM++ outputs 192-dim
                quality_score=min(1.0, duration_ms / 5000),  # Quality improves with duration
                duration_ms=duration_ms,
            )

        except Exception as e:
            logger.error(f"Embedding extraction error: {e}")
            return inference_pb2.EmbeddingResponse(
                embedding=[],
                quality_score=0.0,
                duration_ms=duration_ms,
            )

    def VerifySpeaker(self, request, context):
        """Verify if audio matches a reference embedding using cosine similarity."""
        import numpy as np

        audio_data = request.audio_data
        reference_embedding = np.array(request.reference_embedding)

        # Extract embedding from audio
        extract_result = self.ExtractEmbedding(
            inference_pb2.ExtractEmbeddingRequest(
                audio_data=audio_data,
                sample_rate=16000.0,
            ),
            context,
        )

        if not extract_result.embedding:
            return inference_pb2.VerifySpeakerResponse(
                similarity_score=0.0,
                threshold=0.0,
                accepted=False,
            )

        test_embedding = np.array(extract_result.embedding)

        # Cosine similarity
        similarity = float(np.dot(reference_embedding, test_embedding) /
                          (np.linalg.norm(reference_embedding) * np.linalg.norm(test_embedding) + 1e-8))

        # Z-Norm threshold at 0.0 (raw cosine, Z-Norm applied in Rust)
        raw_threshold = 0.5
        accepted = similarity > raw_threshold

        return inference_pb2.VerifySpeakerResponse(
            similarity_score=similarity,
            threshold=raw_threshold,
            accepted=accepted,
        )

    def HealthCheck(self, request, context):
        """Report model loading status."""
        state = self._model_loader.state
        return inference_pb2.HealthCheckResponse(
            ready=state.asr_ready and state.speaker_ready,
            vad_loaded=state.vad_ready,
            asr_loaded=state.asr_ready,
            speaker_loaded=state.speaker_ready,
            error_detail=state.error_detail,
        )
```

- [x] **Step 5: Update main.py to register all services**

```python
# python-worker/main.py
import asyncio
import logging
from concurrent import futures

import grpc
from grpc_health.v1 import health, health_pb2, health_pb2_grpc

from config import WorkerConfig
from models.loader import ModelLoader
from services.vad import VADService
from services.asr import ASRService
from services.speaker import SpeakerService
import inference_pb2_grpc

logger = logging.getLogger("actio-worker")


async def serve() -> None:
    config = WorkerConfig.from_env()
    logger.info(f"Starting worker on {config.host}:{config.port}")

    # Load models
    model_loader = ModelLoader()

    # Pre-load models (or defer to first request)
    try:
        model_loader.load_asr_model()
        model_loader.load_speaker_model()
    except Exception as e:
        logger.warning(f"Model pre-loading failed (will retry on first request): {e}")

    server = grpc.aio.server(futures.ThreadPoolExecutor(max_workers=10))

    # Health check service
    health_servicer = health.HealthServicer()
    health_pb2_grpc.add_HealthServicer_to_server(health_servicer, server)
    health_servicer.set("", health_pb2.HealthCheckResponse.SERVING)

    # Inference services
    inference_pb2_grpc.add_VADServiceServicer_to_server(VADService(), server)
    inference_pb2_grpc.add_ASRServiceServicer_to_server(ASRService(model_loader), server)
    inference_pb2_grpc.add_SpeakerServiceServicer_to_server(SpeakerService(model_loader), server)

    server.add_insecure_port(f"{config.host}:{config.port}")
    await server.start()
    logger.info(f"Worker started on {config.host}:{config.port}")

    try:
        await server.wait_for_termination()
    except KeyboardInterrupt:
        logger.info("Shutting down worker...")
        await server.stop(grace=5.0)


if __name__ == "__main__":
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )
    asyncio.run(serve())
```

- [x] **Step 6: Update services/__init__.py**

```python
# python-worker/services/__init__.py
from .vad import VADService
from .asr import ASRService
from .speaker import SpeakerService
```

- [x] **Step 7: Test the worker starts**

```bash
cd python-worker
source .venv/bin/activate
timeout 10 python main.py || true
```

Expected: Worker starts, attempts to load models (may fail if not downloaded yet — that's OK for MVP).

- [x] **Step 8: Commit**

```bash
git add python-worker/models/ python-worker/services/ python-worker/main.py
git commit -m "feat: Python Worker VAD, ASR, and Speaker gRPC services with model loader"
```

---

## Execution Order

```
Lane A (Sequential):
  Task 7 → Task 8 → Task 9 → Task 10

Task 7 (Worker Manager) must come first because:
  - gRPC client needs a worker to connect to
  - API routes need the worker manager in AppState

Task 8 (gRPC Client) depends on Task 7:
  - Client needs the proto-generated code

Task 9 (API) depends on Task 8:
  - Endpoints need the gRPC client and inference router

Task 10 (Python Services) depends on Task 8:
  - Services must match the proto definitions
```

---

## Completion State

After completing Plan Part 2, the following should work:
- [x] `cargo check` passes (with engine module stubs for Part 3)
- [x] Python Worker starts with all 3 services registered
-- [x] gRPC health check reports model loading status
- [x] REST endpoints for sessions, speakers, transcripts return proper responses
- [x] Worker manager can start/stop the Python process
- [x] 4 commits with clean history

---

## NOT in scope for Part 2

| Item | Rationale |
|------|-----------|
| Circuit breaker | Part 3, Task 11 |
| Audio stream coordinator | Part 3, Task 12 |
| Speaker matcher (pgvector) | Part 3, Task 13 |
| Transcript aggregator | Part 3, Task 14 |
| WebSocket audio handler | Part 3, Task 15 |
| Integration tests | Part 3, Task 16 |
| Metrics/observability | Part 3, Task 17 |
| Cloud ASR | Phase 2 feature |
| Auth | Phase 3 feature |
