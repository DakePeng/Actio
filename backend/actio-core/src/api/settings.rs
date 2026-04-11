use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::AppState;
use crate::api::session::AppApiError;
use crate::engine::app_settings::{AppSettings, SettingsPatch};
use crate::engine::audio_capture::{self, AudioDeviceInfo};
use crate::engine::model_manager::{AsrModelInfo, DownloadTarget, ModelStatus};

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
pub async fn get_available_models(
    State(state): State<AppState>,
) -> Json<Vec<AsrModelInfo>> {
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
    state
        .model_manager
        .start_download(req.target, tx)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(StatusCode::ACCEPTED)
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
        .map_err(|e| AppApiError(e.to_string()))?;
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
        .map_err(|e| AppApiError(e.to_string()))?;

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
        .map_err(|e| AppApiError(format!("warmup task join error: {e}")))?;

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
    Json(state.settings_manager.update(patch).await)
}

#[derive(Serialize)]
pub struct LlmTestResult {
    pub success: bool,
    pub message: String,
}

/// POST /settings/llm/test — test LLM connectivity with a minimal chat completion request.
pub async fn test_llm(
    State(state): State<AppState>,
) -> Result<Json<LlmTestResult>, StatusCode> {
    let settings = state.settings_manager.get().await;

    let Some(base_url) = &settings.llm.base_url else {
        return Ok(Json(LlmTestResult {
            success: false,
            message: "No LLM base URL configured".into(),
        }));
    };

    let Some(api_key) = &settings.llm.api_key else {
        return Ok(Json(LlmTestResult {
            success: false,
            message: "No API key configured".into(),
        }));
    };

    let client = reqwest::Client::new();
    let model = settings.llm.model.as_deref().unwrap_or("gpt-4o-mini");
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => Ok(Json(LlmTestResult {
            success: true,
            message: format!("Connected to {} (model: {})", base_url, model),
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
