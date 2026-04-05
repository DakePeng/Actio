use tonic::transport::Channel;
use tracing::info;

use crate::error::AppError;
use crate::grpc::*;

#[derive(Clone)]
pub struct GrpcClient {
    channel: Channel,
}

impl GrpcClient {
    pub async fn connect_with_retry(
        addr: &str,
        attempts: usize,
        delay: std::time::Duration,
    ) -> Result<Self, anyhow::Error> {
        let mut last_error = None;

        for attempt in 1..=attempts {
            match Self::connect(addr).await {
                Ok(client) => return Ok(client),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < attempts {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(last_error.expect("connect_with_retry requires at least one attempt"))
    }

    pub async fn connect(addr: &str) -> Result<Self, anyhow::Error> {
        let channel = Channel::from_shared(addr.to_string())?.connect().await?;
        info!(addr, "gRPC client connected");
        Ok(Self { channel })
    }

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

        let response = client.stream_recognize(
            tokio_stream::wrappers::ReceiverStream::new(rx)
        ).await?;
        let mut results = vec![];
        let mut stream = response.into_inner();
        while let Some(result) = stream.message().await? {
            results.push(result);
        }
        Ok(results)
    }

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

    pub async fn health_check(&self) -> Result<HealthCheckResponse, AppError> {
        let mut client = SpeakerServiceClient::new(self.channel.clone());
        let response = client.health_check(HealthCheckRequest {}).await?;
        Ok(response.into_inner())
    }
}
