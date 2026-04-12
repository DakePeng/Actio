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
use crate::engine::llm_downloader::LlmDownloader;
use crate::engine::llm_endpoint::LocalLlmEndpoint;
use crate::engine::llm_router::{LlmRouter, LlmSelection};
use crate::engine::local_llm_engine::EngineSlot;
use crate::engine::metrics::Metrics;
use crate::engine::model_manager::ModelManager;
use crate::engine::remote_llm_client::RemoteLlmClient;
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
    pub model_manager: Arc<ModelManager>,
    pub inference_pipeline: Arc<tokio::sync::Mutex<InferencePipeline>>,
    pub settings_manager: Arc<SettingsManager>,
    /// Signalled by the settings handler when `audio.asr_model` changes.
    /// The pipeline supervisor listens for this and hot-swaps the recognizer.
    pub pipeline_restart: Arc<tokio::sync::Notify>,
    /// Local LLM engine slot (lazy-loaded).
    pub engine_slot: Arc<EngineSlot>,
    /// Downloader for local LLM GGUF files.
    pub llm_downloader: Arc<LlmDownloader>,
    /// Optional fallback remote client constructed from env vars on first launch.
    pub remote_client_envseed: Option<Arc<RemoteLlmClient>>,
    /// Active LLM router. Rebuilt whenever LlmSettings.selection changes.
    pub router: Arc<tokio::sync::RwLock<LlmRouter>>,
    /// Optional second listener for the /v1/* endpoint on a configurable port.
    pub llm_endpoint: Arc<tokio::sync::Mutex<LocalLlmEndpoint>>,
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

    // Shared download mutex enforces "one download at a time" across both
    // ASR (ModelManager) and LLM (LlmDownloader).
    let download_lock = Arc::new(tokio::sync::Mutex::new(()));

    let model_manager = Arc::new(ModelManager::new(config.model_dir.clone()));
    let llm_downloader = Arc::new(LlmDownloader::new(
        config.model_dir.clone(),
        download_lock.clone(),
    ));
    let engine_slot = Arc::new(EngineSlot::new(config.model_dir.clone()));

    let remote_client_envseed = LlmConfig::from_env_optional()
        .map(RemoteLlmClient::new)
        .map(Arc::new);

    let inference_pipeline = Arc::new(tokio::sync::Mutex::new(InferencePipeline::new()));
    let settings_manager = Arc::new(SettingsManager::new(&config.data_dir));

    // Build the initial router from current settings.
    let initial_settings = settings_manager.get().await;
    let initial_router = build_router_from_settings(
        &initial_settings.llm,
        &engine_slot,
        remote_client_envseed.as_ref().cloned(),
    );
    let router = Arc::new(tokio::sync::RwLock::new(initial_router));

    let llm_endpoint = Arc::new(tokio::sync::Mutex::new(LocalLlmEndpoint::new()));

    let state = AppState {
        pool,
        aggregator,
        metrics,
        model_manager,
        inference_pipeline,
        settings_manager,
        pipeline_restart: Arc::new(tokio::sync::Notify::new()),
        engine_slot,
        llm_downloader,
        remote_client_envseed,
        router,
        llm_endpoint,
    };

    // If the configured local_endpoint_port differs from the backend port,
    // start the second listener now.
    let configured_port = initial_settings.llm.local_endpoint_port;
    if configured_port != config.http_port {
        let state_clone = state.clone();
        let mut endpoint = state.llm_endpoint.lock().await;
        if let Err(e) = endpoint.start_or_rebind(configured_port, state_clone).await {
            warn!(port = configured_port, error = %e,
                "Failed to bind local LLM endpoint listener at startup; /v1 routes may be unavailable on configured port");
        }
    }

    // Spawn the always-on inference pipeline. The design intent is that the
    // app listens continuously — UI clients (Recording tab, chat composer
    // dictation) just open a WebSocket subscription to receive the live
    // transcript stream as it's produced. They never start or stop the
    // pipeline themselves.
    //
    // To avoid burning RAM when no UI is listening, a supervisor task watches
    // the broadcast receiver count and hibernates the recognizer (frees the
    // model RAM) after `IDLE_GRACE_PERIOD` of no subscribers, then wakes it
    // back up on the next WebSocket connect.
    {
        let state = state.clone();
        tokio::spawn(async move {
            // Warm start at boot so the first user click is fast.
            if let Err(e) = start_always_on_pipeline(&state).await {
                warn!(error = %e, "Could not warm-start always-on inference pipeline");
            }
            pipeline_supervisor(state).await;
        });
    }

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
        let addr = format!("127.0.0.1:{}", port);
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

/// Construct a fresh `LlmRouter` from the current `LlmSettings`.
pub fn build_router_from_settings(
    llm: &crate::engine::app_settings::LlmSettings,
    engine_slot: &Arc<EngineSlot>,
    remote_envseed: Option<Arc<RemoteLlmClient>>,
) -> LlmRouter {
    match &llm.selection {
        LlmSelection::Disabled => LlmRouter::Disabled,
        LlmSelection::Local { id } => LlmRouter::Local {
            slot: Arc::clone(engine_slot),
            model_id: id.clone(),
        },
        LlmSelection::Remote => {
            if let (Some(base_url), Some(api_key)) = (
                llm.remote.base_url.as_deref(),
                llm.remote.api_key.as_deref(),
            ) {
                let cfg = crate::config::LlmConfig {
                    base_url: base_url.into(),
                    api_key: api_key.into(),
                    model: llm.remote.model.clone().unwrap_or_else(|| "gpt-4o-mini".into()),
                };
                LlmRouter::Remote(Arc::new(RemoteLlmClient::new(cfg)))
            } else if let Some(env_client) = remote_envseed {
                LlmRouter::Remote(env_client)
            } else {
                LlmRouter::Disabled
            }
        }
    }
}

/// How long to wait with zero broadcast subscribers before hibernating
/// the always-on inference pipeline. Brief pauses (changing tabs, finishing
/// a sentence) shouldn't trigger hibernation; sustained idleness should.
const IDLE_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(60);

/// Start the inference pipeline. Idempotent: if a pipeline is already
/// running, this is a no-op. Creates a fresh "always_on" session row in the
/// DB on each call so transcripts produced after a hibernation cycle are
/// grouped under their own session.
///
/// Fails gracefully if no ASR model is downloaded or no audio device is
/// available — the rest of the API still works, WS subscribers just see no
/// transcripts until the user fixes the underlying issue and relaunches.
async fn start_always_on_pipeline(state: &AppState) -> anyhow::Result<()> {
    // Cheap pre-check so we don't even create a session row if we'd just
    // bail. The proper check happens under the lock below.
    {
        let pipeline = state.inference_pipeline.lock().await;
        if pipeline.is_running() {
            return Ok(());
        }
    }

    let model_paths = state
        .model_manager
        .model_paths()
        .await
        .ok_or_else(|| anyhow::anyhow!("models not ready — skipping always-on pipeline"))?;

    let settings = state.settings_manager.get().await;
    let asr_model = settings.audio.asr_model.clone();

    // Tenant must match the dev tenant the frontend filters by, same as
    // the label seeding logic in this file.
    let tenant_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001")
        .expect("hardcoded uuid must parse");

    let session = repository::session::create_session(
        &state.pool,
        tenant_id,
        "microphone",
        "always_on",
    )
    .await?;
    let session_id = session.id.parse::<uuid::Uuid>()?;

    {
        let mut pipeline = state.inference_pipeline.lock().await;
        // Re-check inside the lock to avoid a race where two callers both
        // pass the pre-check, then both create sessions and one's
        // start_session fails because the other already started the pipeline.
        if pipeline.is_running() {
            return Ok(());
        }
        pipeline.start_session(
            session_id,
            &model_paths,
            state.aggregator.clone(),
            None,
            asr_model.as_deref(),
        )?;
    }

    info!(%session_id, model = ?asr_model, "Always-on inference pipeline started");
    Ok(())
}

/// Watches the aggregator's broadcast receiver count and hibernates /
/// resumes the inference pipeline accordingly. Also listens for explicit
/// restart signals from the settings handler (model hot-swap). Runs
/// forever as a tokio task.
///
/// State machine:
///   - **Active**: pipeline running, ≥1 subscriber. No timer.
///   - **Grace**: pipeline running, 0 subscribers since `idle_since`.
///     If `idle_since.elapsed() ≥ IDLE_GRACE_PERIOD`, transition to
///     Hibernated. If a subscriber attaches, transition back to Active.
///   - **Hibernated**: pipeline stopped, model RAM freed. On next
///     subscriber, transition back to Active by calling
///     `start_always_on_pipeline`.
///   - **Hot-swap**: on `pipeline_restart` signal, stop current pipeline
///     immediately and restart with freshly-read settings (new model).
async fn pipeline_supervisor(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut idle_since: Option<std::time::Instant> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // ── Periodic hibernation / wake check ──
                let count = state.aggregator.receiver_count();
                let running = state.inference_pipeline.lock().await.is_running();

                if count > 0 {
                    if idle_since.is_some() {
                        info!("Pipeline subscribers reattached — cancelling hibernation timer");
                        idle_since = None;
                    }
                    if !running {
                        info!("Subscriber connected — waking pipeline from hibernation");
                        if let Err(e) = start_always_on_pipeline(&state).await {
                            warn!(error = %e, "Failed to wake pipeline from hibernation");
                        }
                    }
                    continue;
                }

                // No subscribers.
                if !running {
                    continue;
                }

                match idle_since {
                    None => {
                        idle_since = Some(std::time::Instant::now());
                        info!(
                            grace_secs = IDLE_GRACE_PERIOD.as_secs(),
                            "No subscribers — pipeline will hibernate after grace period"
                        );
                    }
                    Some(since) if since.elapsed() >= IDLE_GRACE_PERIOD => {
                        info!(
                            idle_secs = since.elapsed().as_secs(),
                            "Hibernating inference pipeline to free RAM"
                        );
                        let mut pipeline = state.inference_pipeline.lock().await;
                        pipeline.stop();
                        idle_since = None;
                    }
                    Some(_) => {}
                }
            }

            _ = state.pipeline_restart.notified() => {
                // ── Hot-swap: model changed in settings ──
                info!("Model hot-swap requested — restarting pipeline");
                {
                    let mut pipeline = state.inference_pipeline.lock().await;
                    if pipeline.is_running() {
                        pipeline.stop();
                    }
                }
                // Brief pause so the old spawn_blocking thread can exit and
                // release the audio device before we reopen it.
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                idle_since = None;
                if let Err(e) = start_always_on_pipeline(&state).await {
                    warn!(error = %e, "Failed to restart pipeline after model change");
                }
            }
        }
    }
}
