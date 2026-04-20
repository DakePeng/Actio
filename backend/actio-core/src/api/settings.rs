use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::api::session::AppApiError;
use crate::engine::app_settings::{AppSettings, SettingsPatch};
use crate::engine::audio_capture::{self, AudioDeviceInfo};
use crate::engine::model_manager::{AsrModelInfo, DownloadTarget, ModelStatus};
use crate::AppState;

#[derive(Deserialize)]
pub struct DownloadRequest {
    pub target: DownloadTarget,
}

/// GET /settings/models — returns current ModelStatus as JSON.
pub async fn get_model_status(
    State(state): State<AppState>,
) -> Result<Json<ModelStatus>, AppApiError> {
    let status = state.model_manager.status().await;
    Ok(Json(status))
}

/// GET /settings/models/available — lists ASR models and their download status.
pub async fn get_available_models(State(state): State<AppState>) -> Json<Vec<AsrModelInfo>> {
    Json(state.model_manager.available_asr_models())
}

/// GET /settings/audio-devices — lists available audio input devices.
pub async fn list_audio_devices() -> Json<Vec<AudioDeviceInfo>> {
    Json(audio_capture::list_devices())
}

/// POST /settings/models/download — starts background download, returns 202.
pub async fn start_model_download(
    State(state): State<AppState>,
    Json(req): Json<DownloadRequest>,
) -> Result<StatusCode, AppApiError> {
    let (tx, _rx) = watch::channel(ModelStatus::NotDownloaded);
    let source = state.settings_manager.get().await.audio.download_source;
    state
        .model_manager
        .start_download(req.target, source, tx)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
}

/// POST /settings/models/cancel-download — abort a running ASR model download.
pub async fn cancel_model_download(State(state): State<AppState>) -> StatusCode {
    state.model_manager.cancel_download().await;
    StatusCode::OK
}

#[derive(Deserialize)]
pub struct WarmupRequest {
    pub asr_model: String,
}

#[derive(Serialize)]
pub struct DeleteModelResult {
    pub deleted: u32,
}

/// DELETE /settings/models/:id — remove all files for a model (or "shared"
/// for the Silero VAD tier). Returns the number of files deleted.
pub async fn delete_model(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeleteModelResult>, AppApiError> {
    let deleted = state
        .model_manager
        .delete_model(&id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(DeleteModelResult { deleted }))
}

/// POST /settings/models/warmup — preload ASR model files into OS page cache.
///
/// Walks every file that the given ASR model needs (including Silero VAD for
/// offline models) and reads each one in a background task. Returns only
/// after the files are fully resident in the OS page cache so the client can
/// accurately reflect load progress in the UI.
pub async fn warmup_models(
    State(state): State<AppState>,
    Json(req): Json<WarmupRequest>,
) -> Result<StatusCode, AppApiError> {
    let paths = state
        .model_manager
        .files_to_warmup(&req.asr_model)
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

    let asr_model = req.asr_model;
    let handle = tokio::task::spawn_blocking(move || -> u64 {
        let started = std::time::Instant::now();
        let mut total: u64 = 0;
        for p in &paths {
            if !p.exists() {
                tracing::debug!(file = %p.display(), "Warmup skipped — file missing");
                continue;
            }
            // Read the file into a throwaway buffer so the OS page cache
            // retains the bytes. std::fs::read is simple and synchronous,
            // which is fine inside spawn_blocking.
            match std::fs::read(p) {
                Ok(bytes) => {
                    total += bytes.len() as u64;
                    // drop(bytes) — we only care about the side effect
                }
                Err(e) => {
                    tracing::warn!(file = %p.display(), error = %e, "Warmup read failed");
                }
            }
        }
        tracing::info!(
            model = %asr_model,
            bytes = total,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Model warmup complete"
        );
        total
    });

    handle
        .await
        .map_err(|e| AppApiError::Internal(format!("warmup task join error: {e}")))?;

    Ok(StatusCode::OK)
}

/// GET /settings — returns current AppSettings as JSON.
pub async fn get_settings(State(state): State<AppState>) -> Json<AppSettings> {
    Json(state.settings_manager.get().await)
}

/// PATCH /settings — partial update of AppSettings.
pub async fn patch_settings(
    State(state): State<AppState>,
    Json(patch): Json<SettingsPatch>,
) -> Json<AppSettings> {
    // Snapshot before applying the patch.
    // Snapshot all audio fields that affect speaker identification. Any
    // change in this tuple warrants a pipeline restart because SpeakerIdConfig
    // is copied by value into spawned tasks — a running pipeline will not see
    // threshold or window edits until it restarts.
    let old = state.settings_manager.get().await;
    let old_speaker_tuple = (
        old.audio.asr_model.clone(),
        old.audio.speaker_confirm_threshold,
        old.audio.speaker_tentative_threshold,
        old.audio.speaker_min_duration_ms,
        old.audio.speaker_continuity_window_ms,
    );
    let llm_changed = patch.llm.is_some();
    let new_settings = state.settings_manager.update(patch).await;
    let new_speaker_tuple = (
        new_settings.audio.asr_model.clone(),
        new_settings.audio.speaker_confirm_threshold,
        new_settings.audio.speaker_tentative_threshold,
        new_settings.audio.speaker_min_duration_ms,
        new_settings.audio.speaker_continuity_window_ms,
    );

    if old_speaker_tuple != new_speaker_tuple {
        tracing::info!(
            ?old_speaker_tuple,
            ?new_speaker_tuple,
            "Speaker-ID settings changed — signalling pipeline restart"
        );
        state.pipeline_restart.notify_one();
    }

    if llm_changed {
        // Rebuild the router. If transitioning away from Local, unload
        // the engine to release RAM.
        let new_router = crate::build_router_from_settings(
            &new_settings.llm,
            &state.engine_slot,
            state.remote_client_envseed.clone(),
        );
        let was_local = state.router.read().await.is_local();
        let now_local = new_router.is_local();
        if was_local && !now_local {
            state.engine_slot.unload().await;
            tracing::info!("LLM selection changed away from Local — unloaded engine");
        }
        *state.router.write().await = new_router;

        // Rebind the separate /v1 LLM endpoint if the port changed.
        let new_port = new_settings.llm.local_endpoint_port;
        let mut endpoint = state.llm_endpoint.lock().await;
        if endpoint.bound_port() != Some(new_port) {
            if let Err(e) = endpoint.start_or_rebind(new_port, state.clone()).await {
                tracing::warn!(port = new_port, error = %e, "Failed to rebind LLM endpoint");
            }
        }
    }

    Json(new_settings)
}

#[derive(Serialize)]
pub struct LlmTestResult {
    pub success: bool,
    pub message: String,
}

/// POST /settings/llm/test — test the active LLM backend with a tiny prompt.
pub async fn test_llm(State(state): State<AppState>) -> Result<Json<LlmTestResult>, StatusCode> {
    use crate::engine::llm_prompt::ChatMessage;
    use crate::engine::llm_router::LlmSelection;
    use crate::engine::local_llm_engine::{EnginePriority, GenerationParams};

    let settings = state.settings_manager.get().await;
    let started = std::time::Instant::now();
    tracing::info!(selection = ?settings.llm.selection, "test_llm: starting");

    match &settings.llm.selection {
        LlmSelection::Disabled => Ok(Json(LlmTestResult {
            success: false,
            message: "No LLM backend selected. Pick Local or Remote in Settings.".into(),
        })),
        LlmSelection::Local { id } => {
            tracing::info!(llm_id = %id, "test_llm: loading local engine");
            let engine = match state.engine_slot.get_or_load(id).await {
                Ok(e) => e,
                Err(e) => {
                    return Ok(Json(LlmTestResult {
                        success: false,
                        message: format!("Failed to load {id}: {e}"),
                    }));
                }
            };
            let messages = vec![ChatMessage {
                role: "user".into(),
                content: "Reply with the single word 'ok' and nothing else.".into(),
            }];
            tracing::info!(llm_id = %id, "test_llm: sending test prompt");
            match engine
                .chat_completion(
                    messages,
                    GenerationParams {
                        max_tokens: 8,
                        temperature: 0.0,
                        json_mode: false,
                        thinking_budget: None,
                    },
                    EnginePriority::Internal,
                )
                .await
            {
                Ok(resp) => {
                    tracing::info!(llm_id = %id, elapsed_ms = started.elapsed().as_millis() as u64, response = %resp.trim(), "test_llm: success");
                    Ok(Json(LlmTestResult {
                        success: true,
                        message: format!(
                            "{} responded in {} ms: {}",
                            engine.metadata().name,
                            started.elapsed().as_millis(),
                            resp.trim(),
                        ),
                    }))
                }
                Err(e) => {
                    tracing::warn!(llm_id = %id, error = %e, elapsed_ms = started.elapsed().as_millis() as u64, "test_llm: inference failed");
                    Ok(Json(LlmTestResult {
                        success: false,
                        message: format!("{}: {e}", engine.metadata().name),
                    }))
                }
            }
        }
        LlmSelection::Remote => {
            let Some(base_url) = &settings.llm.remote.base_url else {
                return Ok(Json(LlmTestResult {
                    success: false,
                    message: "Remote selected but no base URL configured".into(),
                }));
            };
            let Some(api_key) = &settings.llm.remote.api_key else {
                return Ok(Json(LlmTestResult {
                    success: false,
                    message: "Remote selected but no API key configured".into(),
                }));
            };
            let model = settings
                .llm
                .remote
                .model
                .as_deref()
                .unwrap_or("gpt-4o-mini");
            let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
            let client = reqwest::Client::new();

            match client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&serde_json::json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Reply with the single word 'ok' and nothing else."}],
                    "max_tokens": 8
                }))
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => Ok(Json(LlmTestResult {
                    success: true,
                    message: format!(
                        "Connected to {} (model: {}) in {} ms",
                        base_url,
                        model,
                        started.elapsed().as_millis()
                    ),
                })),
                Ok(resp) => Ok(Json(LlmTestResult {
                    success: false,
                    message: format!(
                        "HTTP {}: {}",
                        resp.status(),
                        resp.text().await.unwrap_or_default()
                    ),
                })),
                Err(e) => Ok(Json(LlmTestResult {
                    success: false,
                    message: format!("Connection failed: {}", e),
                })),
            }
        }
    }
}
