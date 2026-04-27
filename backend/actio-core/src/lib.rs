pub mod api;
pub mod config;
pub mod domain;
pub mod engine;
pub mod repository;

#[cfg(test)]
pub mod testing;

use std::path::PathBuf;
use std::sync::Arc;

use axum::http::{HeaderName, Method};
use sqlx::SqlitePool;
use tower_http::cors::{AllowOrigin, CorsLayer};
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
    /// New batch-clip-processing pipeline. Lifecycle handle for the
    /// claim → process → mark loop. Spawned at boot (gated on
    /// `always_listening`) and stopped when the user toggles privacy
    /// mode. The legacy InferencePipeline above remains the live path
    /// until the streaming-loop port from `engine::live_streaming`
    /// completes — both pipelines coexist by design.
    pub batch_processor: Arc<crate::engine::batch_processor::BatchProcessorHandle>,
    /// Always-on capture daemon (cpal + Silero VAD) shared by the new
    /// batch pipeline and (eventually) the live streaming service.
    /// Constructed at boot but only started by the new supervisor when
    /// `always_listening = true` AND the user is unmuted.
    pub capture_daemon: Arc<crate::engine::capture_daemon::CaptureDaemon>,
    /// On-demand live streaming service (dictation + translation). Same
    /// note as `inference_pipeline` — it ships as a skeleton; flipping
    /// dictation/translation handlers from `inference_pipeline` to here
    /// is a follow-up.
    pub live_streaming: Arc<crate::engine::live_streaming::LiveStreamingService>,
    pub settings_manager: Arc<SettingsManager>,
    /// Directory where Phase-A voiceprint-candidate clips are written.
    pub clips_dir: PathBuf,
    /// Shared state for live voiceprint enrollment. When Some+Active, the
    /// pipeline routes quality-passing VAD segments into the target
    /// speaker's voiceprints instead of the normal identify/candidate path.
    pub live_enrollment: crate::engine::live_enrollment::LiveEnrollment,
    /// Tracks the session UUID the enrollment flow started the pipeline
    /// with, when no transcription session was already running. Lets the
    /// cancel handler stop exactly what it started and leave real
    /// transcription sessions untouched.
    pub enrollment_owned_session: Arc<tokio::sync::Mutex<Option<uuid::Uuid>>>,
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
    /// Process-level mutex serializing all `LlmRouter` calls (window
    /// extraction + translation). Tokio's mutex is FIFO so neither
    /// feature starves.
    pub llm_inflight: std::sync::Arc<tokio::sync::Mutex<()>>,
    /// Broadcast of EMA-smoothed mic RMS, sampled per audio chunk
    /// (~20–60ms). The WS handler subscribes and emits `audio_level`
    /// events so the live tab's voice wave reflects real input.
    /// Lossy by design — receivers that fall behind drop frames.
    pub audio_levels: Arc<tokio::sync::broadcast::Sender<f32>>,
    /// Optional second listener for the /v1/* endpoint on a configurable port.
    pub llm_endpoint: Arc<tokio::sync::Mutex<LocalLlmEndpoint>>,
}

/// Initialize tracing with two writers: stderr (visible in dev) and a
/// rolling daily log file under `<data_dir>/logs/actio.log`. The file writer
/// is what keeps logs alive for bundled `.exe` / `.app` / AppImage launches —
/// stderr is silently discarded when the OS attaches no console.
fn init_tracing(data_dir: &std::path::Path) {
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    let log_dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "actio.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    // Leak the guard so the background flush thread keeps draining writes
    // for the program's lifetime. Dropping the guard joins on the worker,
    // which would lose the last few log lines on shutdown.
    Box::leak(Box::new(guard));

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "actio_core=info".parse().unwrap());

    // ANSI off globally — escape codes are unreadable in the file. The minor
    // loss of color in the dev terminal is an acceptable trade for grep-able
    // production logs the user can attach to a bug report.
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(file_writer.and(std::io::stderr))
        .with_ansi(false)
        .init();
}

/// Start the Axum HTTP server. Called from Tauri's setup hook.
pub async fn start_server(config: CoreConfig) -> anyhow::Result<()> {
    init_tracing(&config.data_dir);

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

    let model_manager = Arc::new(ModelManager::new(config.model_dir.clone()));
    let llm_downloader = Arc::new(LlmDownloader::new(config.model_dir.clone()));
    let engine_slot = Arc::new(EngineSlot::new(config.model_dir.clone()));

    let remote_client_envseed = LlmConfig::from_env_optional()
        .map(RemoteLlmClient::new)
        .map(Arc::new);

    let inference_pipeline = Arc::new(tokio::sync::Mutex::new(InferencePipeline::new()));
    let settings_manager = Arc::new(SettingsManager::new(&config.data_dir));

    // New batch-clip-processing pipeline scaffolding. The handles are
    // constructed unconditionally so the API and supervisor have stable
    // references; whether they actually run is decided each tick by the
    // batch supervisor below based on `always_listening`.
    let batch_processor = Arc::new(crate::engine::batch_processor::BatchProcessorHandle::new());
    let capture_daemon = {
        // Default device for now — when audio.device_name is wired through
        // settings updates, the daemon's start() will pick it up via the
        // Inner.device_name field. The Silero VAD model lives next to the
        // other ONNX models the model_manager already extracts on first run.
        let initial_device = settings_manager.get().await.audio.device_name.clone();
        let vad_path = config.model_dir.join("silero_vad.onnx");
        Arc::new(crate::engine::capture_daemon::CaptureDaemon::new(
            initial_device,
            vad_path,
        ))
    };
    let live_streaming = Arc::new(crate::engine::live_streaming::LiveStreamingService::new(
        capture_daemon.clone(),
        aggregator.clone(),
    ));

    // Phase-A voiceprint-candidate retention: clips land in <data_dir>/audio_clips/
    // and get swept hourly per the user's `audio.clip_retention_days` setting.
    let clips_dir = config.data_dir.join("audio_clips");
    if let Err(e) = std::fs::create_dir_all(&clips_dir) {
        warn!(error = %e, ?clips_dir, "failed to create voiceprint-clip dir");
    }
    {
        let retention_days = settings_manager.get().await.audio.clip_retention_days;
        crate::engine::clip_storage::start_cleanup_task(clips_dir.clone(), retention_days);
    }

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
        batch_processor,
        capture_daemon,
        live_streaming,
        settings_manager,
        clips_dir,
        live_enrollment: crate::engine::live_enrollment::new_state(),
        enrollment_owned_session: Arc::new(tokio::sync::Mutex::new(None)),
        pipeline_restart: Arc::new(tokio::sync::Notify::new()),
        engine_slot,
        llm_downloader,
        remote_client_envseed,
        router,
        llm_inflight: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        audio_levels: Arc::new(tokio::sync::broadcast::channel::<f32>(32).0),
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

    // If load_on_startup is enabled and a local model is selected with a
    // cached UQFF file, start loading it in the background.
    // If no UQFF exists, skip — don't trigger a download at startup.
    if initial_settings.llm.load_on_startup {
        if let crate::engine::llm_router::LlmSelection::Local { ref id } =
            initial_settings.llm.selection
        {
            let info = crate::engine::llm_catalog::available_local_llms()
                .into_iter()
                .find(|m| m.id == *id);
            if let Some(info) = info {
                if crate::engine::local_llm_engine::LocalLlmEngine::has_gguf(
                    &config.model_dir,
                    &info,
                ) {
                    info!(llm_id = %id, "Loading cached local LLM at startup");
                    state
                        .engine_slot
                        .start_load(
                            id.clone(),
                            initial_settings.llm.download_source,
                            &state.llm_downloader,
                        )
                        .await;
                } else {
                    info!(llm_id = %id, "Skipping startup load — no cached GGUF, model needs download first");
                }
            }
        }
    }

    // Pick the always-on pipeline based on the user's opt-in. The two paths
    // are mutually exclusive — both would try to grab the microphone.
    let use_batch_pipeline = state.settings_manager.get().await.audio.use_batch_pipeline;
    if use_batch_pipeline {
        info!("Always-on pipeline: batch clip processing (new)");
        let state_clone = state.clone();
        tokio::spawn(async move {
            if let Err(e) = start_always_on_capture(&state_clone).await {
                warn!(error = %e, "Could not warm-start always-on capture daemon");
            }
        });
    } else {
        // Legacy: spawn the InferencePipeline supervisor that watches the
        // broadcast receiver count and hibernates after IDLE_GRACE_PERIOD.
        info!("Always-on pipeline: legacy InferencePipeline");
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = start_always_on_pipeline(&state).await {
                warn!(error = %e, "Could not warm-start always-on inference pipeline");
            }
            pipeline_supervisor(state).await;
        });
    }

    // Background windowed action-item extractor. Runs every
    // `extraction_tick_secs` and drains one pending window per tick.
    {
        let state = state.clone();
        tokio::spawn(async move {
            crate::engine::window_extractor::run_extraction_loop(state).await;
        });
    }

    // ── Batch clip processing background tasks ──────────────────────
    //
    // Three background lifecycles, all gated on user-visible privacy:
    //
    //   1. Nested clip-dir cleanup — sweeps <clips_dir>/<session>/<clip>/
    //      every hour, removes whole clip directories older than
    //      `audio.audio_retention_days` (default 14). Distinct from the
    //      legacy flat-dir voiceprint candidate sweep above; both run
    //      until Plan Task 17 retires the legacy infra.
    //
    //   2. Provisional speaker GC — daily DELETE of provisional rows
    //      whose `provisional_last_matched_at` is older than
    //      `audio.provisional_voiceprint_gc_days` (default 30).
    //
    //   3. Batch supervisor — every 5 s, syncs CaptureDaemon's
    //      `archive_enabled` flag to `audio.always_listening`. This is
    //      the privacy-mode hook (Plan Task 13): when always_listening is
    //      false, the clip writer drops speech events instead of writing
    //      them to disk. Live streaming subscribers still receive events.
    //      Spawning the BatchProcessorHandle's claim loop is left to the
    //      supervisor refactor that flips the live path off
    //      InferencePipeline; until then, the BatchProcessor only runs if
    //      the user-side has explicitly populated audio_clips rows.
    {
        let initial = settings_manager_ref(&state);
        let clip_dir_root = state.clips_dir.clone();
        crate::engine::clip_storage::start_clip_dir_cleanup_task(
            clip_dir_root,
            initial.audio.audio_retention_days,
        );
    }
    {
        let pool = state.pool.clone();
        let gc_days = state
            .settings_manager
            .get()
            .await
            .audio
            .provisional_voiceprint_gc_days as i64;
        crate::engine::clip_storage::start_provisional_speaker_gc_task(pool, gc_days);
    }
    {
        let state = state.clone();
        tokio::spawn(async move {
            batch_pipeline_supervisor(state).await;
        });
    }

    // Admit:
    //   - dev-server origins (vite on 5173, Tauri-dev wrapper on 1420)
    //   - production Tauri WebView schemes:
    //       Windows WebView2 → "https://tauri.localhost"
    //       macOS WKWebView / Linux WebKitGTK → "tauri://localhost"
    //   - any localhost-bound HTTP origin (covers port-conflict fallback to
    //     3001-3009 and other dev-time tooling on the loopback)
    // A predicate is preferred over a static list because Tauri's macOS/Linux
    // builds use a custom URL scheme that tower-http's HeaderValue parser
    // can otherwise reject as invalid.
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _req| {
            let s = match origin.to_str() {
                Ok(s) => s,
                Err(_) => return false,
            };
            s == "tauri://localhost"
                || s == "https://tauri.localhost"
                || s.starts_with("http://localhost")
                || s.starts_with("http://127.0.0.1")
        }))
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
        anyhow::bail!(
            "Could not bind to any port in range {}-{}",
            config.http_port,
            config.http_port + 9
        );
    }

    Ok(())
}

/// Start the new batch-clip-processing always-on path:
///   1. Create a default session if none exists for this boot.
///   2. Start the CaptureDaemon (cpal + Silero VAD).
///   3. Spawn the ClipWriter loop tied to that session — it subscribes to
///      the daemon's broadcast, runs the boundary state machine, writes
///      per-segment WAVs + manifest, and inserts audio_clips rows.
///
/// The BatchProcessor's claim loop is driven by the existing
/// `batch_pipeline_supervisor`, which calls
/// `BatchProcessorHandle::ensure_running` whenever
/// `audio.always_listening = true`. Together this is the new default
/// pipeline — opt in via `audio.use_batch_pipeline`.
async fn start_always_on_capture(state: &AppState) -> anyhow::Result<()> {
    let settings = state.settings_manager.get().await;
    if !settings.audio.always_listening {
        info!("always_listening=false at boot — skipping capture daemon start");
        return Ok(());
    }

    // Tenant must match the dev tenant the frontend filters by — same as
    // the legacy start_always_on_pipeline.
    let tenant_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001")
        .expect("hardcoded uuid must parse");

    // Reuse the latest still-open always_on session if one exists, so a
    // crash + restart doesn't accumulate orphan rows in audio_sessions.
    // Fall through to create_session only when no open row exists.
    let session_id = match sqlx::query_as::<_, (String,)>(
        "SELECT id FROM audio_sessions \
         WHERE tenant_id = ?1 \
           AND mode = 'always_on' \
           AND ended_at IS NULL \
         ORDER BY started_at DESC \
         LIMIT 1",
    )
    .bind(tenant_id.to_string())
    .fetch_optional(&state.pool)
    .await?
    {
        Some((id_str,)) => uuid::Uuid::parse_str(&id_str)?,
        None => {
            let session = repository::session::create_session(
                &state.pool,
                tenant_id,
                "microphone",
                "always_on",
            )
            .await?;
            session.id.parse::<uuid::Uuid>()?
        }
    };

    state.capture_daemon.start().await?;

    // Spawn the clip writer loop. It runs until the broadcast channel
    // closes (daemon shutdown) or the receiver lags severely. On a clean
    // app exit this just gets dropped along with the rest of the runtime.
    let pool = state.pool.clone();
    let daemon = state.capture_daemon.clone();
    let cfg = crate::engine::clip_writer::ClipWriterConfig {
        clips_dir: state.clips_dir.clone(),
        boundary: crate::engine::clip_boundary::BoundaryConfig {
            target_secs: settings.audio.clip_target_secs,
            max_secs: settings.audio.clip_max_secs,
            silence_close_ms: settings.audio.clip_close_silence_ms,
        },
    };
    let events = state.capture_daemon.subscribe();
    tokio::spawn(async move {
        crate::engine::clip_writer::run_clip_writer_loop(pool, session_id, cfg, daemon, events)
            .await;
    });

    info!(%session_id, "Always-on capture + clip writer started");
    Ok(())
}

/// Convenience: snapshot the current `AppSettings` synchronously when the
/// caller already holds `state`. Used by start_server so the cleanup task
/// can read its retention setting once at spawn time without forcing the
/// caller to await inside the spawn block.
fn settings_manager_ref(state: &AppState) -> crate::engine::app_settings::AppSettings {
    // settings_manager.get() is async; we use try_read on the inner RwLock
    // when we know the caller is in startup (no contention). Falling back
    // to defaults is harmless — the user can always patch settings later.
    futures::executor::block_on(state.settings_manager.get())
}

/// Batch pipeline supervisor. Runs every 5 s; owns the privacy-mode flip
/// for the new pipeline by syncing CaptureDaemon's `archive_enabled` to
/// `audio.always_listening`. Compared to `pipeline_supervisor` above (the
/// legacy live-path supervisor), this one is intentionally minimal — the
/// real claim-loop spawn lands when the live-path migration completes.
async fn batch_pipeline_supervisor(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    info!("Batch pipeline supervisor started");

    // Construct the ProductionClipRunner once. It re-reads settings each
    // run_clip call so model selection + thresholds can change without a
    // restart.
    let runner: Arc<crate::engine::batch_processor::ProductionClipRunner> =
        Arc::new(crate::engine::batch_processor::ProductionClipRunner {
            settings_manager: state.settings_manager.clone(),
            model_manager: state.model_manager.clone(),
            router: state.router.clone(),
        });

    loop {
        interval.tick().await;
        let always_listening = state.settings_manager.get().await.audio.always_listening;

        // Privacy-mode gate for the archive: when off, the clip writer
        // drops speech events (live subscribers still get them via the
        // CaptureDaemon broadcast).
        state
            .capture_daemon
            .set_archive_enabled(always_listening)
            .await;

        // Drive the BatchProcessor's claim-loop lifecycle. The handle is
        // idempotent — repeat ensure_running calls are no-ops when the
        // loop is already up.
        if always_listening {
            state
                .batch_processor
                .ensure_running(state.pool.clone(), runner.clone())
                .await;
        } else {
            state.batch_processor.ensure_stopped().await;
        }
    }
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
                    model: llm
                        .remote
                        .model
                        .clone()
                        .unwrap_or_else(|| "gpt-4o-mini".into()),
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

/// Legacy: how long to wait with zero broadcast subscribers before
/// hibernating the inference pipeline. Only relevant when
/// `audio.always_listening = false` AND a WS subscriber temporarily
/// attached — we don't want a tab-switch flicker to thrash the pipeline.
/// User-driven mute (mic toggle off) bypasses this entirely and stops
/// the pipeline immediately so the microphone isn't hot for an extra
/// minute after the user said "stop".
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

    let settings = state.settings_manager.get().await;
    let asr_model = settings.audio.asr_model.clone();
    let embedding_id = settings.audio.speaker_embedding_model.clone();
    let model_paths = state
        .model_manager
        .model_paths(embedding_id.as_deref())
        .await
        .ok_or_else(|| anyhow::anyhow!("models not ready — skipping always-on pipeline"))?;

    // Tenant must match the dev tenant the frontend filters by, same as
    // the label seeding logic in this file.
    let tenant_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001")
        .expect("hardcoded uuid must parse");

    let session =
        repository::session::create_session(&state.pool, tenant_id, "microphone", "always_on")
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
            tenant_id,
            &model_paths,
            state.aggregator.clone(),
            None,
            asr_model.as_deref(),
            settings.language.as_deref(),
            state.clips_dir.clone(),
            state.live_enrollment.clone(),
            crate::engine::inference_pipeline::SpeakerIdConfig {
                confirm_threshold: settings.audio.speaker_confirm_threshold,
                tentative_threshold: settings.audio.speaker_tentative_threshold,
                min_duration_ms: settings.audio.speaker_min_duration_ms,
                continuity_window_ms: settings.audio.speaker_continuity_window_ms,
            },
            Some(state.audio_levels.clone()),
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
    // Track the last-seen always_listening value so we can detect the
    // user-driven mute transition (true → false) and stop the pipeline
    // immediately, instead of leaving the mic hot for the legacy
    // 60-second grace period.
    let mut prev_always_listening: Option<bool> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // ── Periodic hibernation / wake check ──
                let always_listening = state.settings_manager.get().await.audio.always_listening;
                let count = state.aggregator.receiver_count();
                let running = state.inference_pipeline.lock().await.is_running();

                // Always-listening mode: pipeline stays up whenever the
                // process is up so the window extractor has transcripts to
                // read. Subscriber count is irrelevant in this branch.
                if always_listening {
                    if idle_since.is_some() {
                        idle_since = None;
                    }
                    if !running {
                        info!("Always-listening on and pipeline stopped — starting");
                        if let Err(e) = start_always_on_pipeline(&state).await {
                            warn!(error = %e, "Failed to start always-listening pipeline");
                        }
                    }
                    prev_always_listening = Some(true);
                    continue;
                }

                // User just flipped the mic toggle off — stop immediately
                // rather than running out the legacy 60s grace period.
                // The user expected the mic to be off the moment they
                // clicked, not a minute later.
                if prev_always_listening == Some(true) && running {
                    info!("Listening disabled by user — stopping pipeline");
                    state.inference_pipeline.lock().await.stop();
                    idle_since = None;
                    prev_always_listening = Some(false);
                    continue;
                }
                prev_always_listening = Some(false);

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

                // No WS subscribers AND user has not just muted (legacy
                // grace path — applies only when always_listening was
                // already false and a WS client temporarily attached).
                if !running {
                    continue;
                }

                match idle_since {
                    None => {
                        idle_since = Some(std::time::Instant::now());
                        info!(
                            grace_secs = IDLE_GRACE_PERIOD.as_secs(),
                            "WS subscribers gone — pipeline will hibernate after grace period"
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
