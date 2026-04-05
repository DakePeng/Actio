mod api;
mod config;
mod domain;
mod engine;
mod error;
mod grpc;
mod repository;

use std::sync::Arc;

use anyhow;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::config::{Config, LlmConfig};
use crate::engine::audio_coordinator::AudioCoordinator;
use crate::engine::circuit_breaker::CircuitBreaker;
use crate::engine::grpc_client::GrpcClient;
use crate::engine::inference_router::InferenceRouter;
use crate::engine::llm_client::LlmClient;
use crate::engine::metrics::Metrics;
use crate::engine::transcript_aggregator::TranscriptAggregator;
use crate::engine::worker::WorkerManager;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub coordinator: Arc<AudioCoordinator>,
    pub aggregator: Arc<TranscriptAggregator>,
    pub circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    pub inference_router: Option<Arc<InferenceRouter>>,
    pub metrics: Arc<Metrics>,
    pub llm_client: Option<Arc<LlmClient>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("actio_asr=info".parse()?),
        )
        .init();

    dotenvy::dotenv().ok();
    let config = Config::from_env();

    let pool = repository::db::create_pool(&config.database_url).await?;
    repository::db::run_migrations(&pool).await?;

    let coordinator = Arc::new(AudioCoordinator::new());
    let aggregator = Arc::new(TranscriptAggregator::new(pool.clone()));
    let circuit_breaker = Arc::new(Mutex::new(CircuitBreaker::new()));
    let metrics = Arc::new(Metrics::new());
    let llm_client = LlmConfig::from_env_optional().map(LlmClient::new).map(Arc::new);

    // Start Python Worker
    let worker_manager = WorkerManager::new(config.worker_host.clone(), config.worker_port);
    if let Err(e) = worker_manager.start().await {
        warn!("Python Worker failed to start: {}", e);
    } else {
        info!("Python worker started on port {}", config.worker_port);
    }

    let grpc_addr = format!("http://{}:{}", config.worker_host, config.worker_port);
    let inference_router = match GrpcClient::connect_with_retry(
        &grpc_addr,
        5,
        std::time::Duration::from_secs(1),
    )
    .await
    {
        Ok(client) => Some(Arc::new(InferenceRouter::new(client, circuit_breaker.clone()))),
        Err(error) => {
            warn!(%grpc_addr, %error, "Worker gRPC client unavailable; realtime transcription disabled");
            None
        }
    };

    let state = AppState {
        pool: pool.clone(),
        coordinator,
        aggregator,
        circuit_breaker,
        inference_router,
        metrics,
        llm_client,
    };

    // Build and start HTTP server
    let app = api::router(state);
    let addr = format!("0.0.0.0:{}", config.http_port);
    info!(%addr, "Starting HTTP server");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    info!("Shutdown signal received");
}
