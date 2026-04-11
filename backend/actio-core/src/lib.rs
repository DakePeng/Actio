pub mod api;
pub mod config;
pub mod domain;
pub mod engine;
pub mod error;
pub mod repository;

use std::path::PathBuf;
use std::sync::Arc;

use axum::http::{HeaderName, Method};
use sqlx::SqlitePool;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::config::LlmConfig;
use crate::engine::app_settings::SettingsManager;
use crate::engine::inference_pipeline::InferencePipeline;
use crate::engine::llm_client::LlmClient;
use crate::engine::metrics::Metrics;
use crate::engine::model_manager::ModelManager;
use crate::engine::transcript_aggregator::TranscriptAggregator;

/// Configuration passed from the Tauri shell to actio-core.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub model_dir: PathBuf,
    pub http_port: u16,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub aggregator: Arc<TranscriptAggregator>,
    pub metrics: Arc<Metrics>,
    pub llm_client: Option<Arc<LlmClient>>,
    pub model_manager: Arc<ModelManager>,
    pub inference_pipeline: Arc<tokio::sync::Mutex<InferencePipeline>>,
    pub settings_manager: Arc<SettingsManager>,
}

/// Start the Axum HTTP server. Called from Tauri's setup hook.
pub async fn start_server(config: CoreConfig) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "actio_core=info".parse().unwrap()),
        )
        .init();

    let db_url = format!("sqlite:{}?mode=rwc", config.db_path.display());
    let pool = repository::db::create_pool(&db_url).await?;
    repository::db::run_migrations(&pool).await?;

    // Seed default labels on first launch (no-op if any labels already exist).
    // Tenant UUID must match the frontend's DEV_TENANT_ID — see
    // frontend/src/api/actio-api.ts. The frontend sends this in the
    // x-tenant-id header on every request, so labels stored under any other
    // tenant would be invisible to the UI even though the API responds.
    let default_tenant = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001")
        .expect("hardcoded uuid must parse");
    match repository::label::seed_default_labels(&pool, default_tenant).await {
        Ok(0) => {} // already seeded or user has custom labels
        Ok(n) => info!(count = n, "Seeded default labels for first launch"),
        Err(e) => warn!(error = %e, "Failed to seed default labels"),
    }

    let aggregator = Arc::new(TranscriptAggregator::new(pool.clone()));
    let metrics = Arc::new(Metrics::new());
    let llm_client = LlmConfig::from_env_optional().map(LlmClient::new).map(Arc::new);
    let model_manager = Arc::new(ModelManager::new(config.model_dir.clone()));

    let inference_pipeline = Arc::new(tokio::sync::Mutex::new(InferencePipeline::new()));
    let settings_manager = Arc::new(SettingsManager::new(&config.data_dir));

    let state = AppState {
        pool,
        aggregator,
        metrics,
        llm_client,
        model_manager,
        inference_pipeline,
        settings_manager,
    };

    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:1420".parse().unwrap(),
            "http://127.0.0.1:1420".parse().unwrap(),
            "http://localhost:5173".parse().unwrap(),
            "http://127.0.0.1:5173".parse().unwrap(),
        ])
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("x-tenant-id"),
        ]);

    let app = api::router(state).layer(cors);

    // Try ports 3000-3009
    let mut bound_port = None;
    for port in config.http_port..config.http_port + 10 {
        let addr = format!("0.0.0.0:{}", port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                info!(%addr, "Actio HTTP server started");
                bound_port = Some(port);
                axum::serve(listener, app).await?;
                break;
            }
            Err(e) => {
                warn!(port, %e, "Port unavailable, trying next");
            }
        }
    }

    if bound_port.is_none() {
        anyhow::bail!("Could not bind to any port in range {}-{}", config.http_port, config.http_port + 9);
    }

    Ok(())
}
