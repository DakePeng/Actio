use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

use crate::engine::circuit_breaker::CircuitBreaker;
use crate::engine::grpc_client::GrpcClient;
use crate::error::AppError;
use crate::grpc::*;

pub struct InferenceRouter {
    grpc_client: GrpcClient,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
}

impl InferenceRouter {
    pub fn new(grpc_client: GrpcClient, circuit_breaker: Arc<Mutex<CircuitBreaker>>) -> Self {
        Self {
            grpc_client,
            circuit_breaker,
        }
    }

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

    pub async fn route_extract_embedding(
        &self,
        audio_data: Vec<u8>,
        sample_rate: f32,
    ) -> Result<EmbeddingResponse, AppError> {
        self.grpc_client.extract_embedding(audio_data, sample_rate).await
    }

    pub async fn route_verify_speaker(
        &self,
        audio_data: Vec<u8>,
        reference_embedding: Vec<f32>,
    ) -> Result<VerifySpeakerResponse, AppError> {
        self.grpc_client.verify_speaker(audio_data, reference_embedding).await
    }
}
