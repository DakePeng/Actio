use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::session::AppApiError;
use crate::engine::llm_catalog::LocalLlmInfo;
use crate::engine::local_llm_engine::LoadStatus;
use crate::AppState;

// ---------------------------------------------------------------------------
// /settings/llm/* routes
// ---------------------------------------------------------------------------

/// GET /settings/llm/models — model catalog.
pub async fn list_local_llms(State(state): State<AppState>) -> Json<Vec<LocalLlmInfo>> {
    Json(state.llm_downloader.catalog())
}

#[derive(Deserialize)]
pub struct LoadLlmRequest {
    pub llm_id: String,
}

/// POST /settings/llm/load — start loading a model in the background.
pub async fn start_llm_load(
    State(state): State<AppState>,
    Json(req): Json<LoadLlmRequest>,
) -> Result<StatusCode, AppApiError> {
    let source = state.settings_manager.get().await.llm.download_source;
    state
        .engine_slot
        .start_load(req.llm_id, source, &state.llm_downloader)
        .await;
    Ok(StatusCode::ACCEPTED)
}

/// POST /settings/llm/cancel-load — cancel an in-progress load.
pub async fn cancel_llm_load(State(state): State<AppState>) -> StatusCode {
    state.engine_slot.cancel_load().await;
    StatusCode::OK
}

/// GET /settings/llm/load-status — current loading state.
pub async fn llm_load_status(State(state): State<AppState>) -> Json<LoadStatus> {
    Json(state.engine_slot.load_status().await)
}

#[derive(Serialize)]
pub struct DeleteLlmResult {
    pub deleted: bool,
}

/// DELETE /settings/llm/models/:id — atomic: unload engine, clear selection, delete files.
pub async fn delete_local_llm(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DeleteLlmResult>, AppApiError> {
    // If this model is currently loaded, unload first.
    if state.engine_slot.loaded_id().await.as_deref() == Some(id.as_str()) {
        state.engine_slot.unload().await;
        tracing::info!(llm_id = %id, "Unloaded engine before deleting model files");
    }

    state
        .llm_downloader
        .delete(&id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

    // If the deleted model was the active selection, switch to Disabled
    // and rebuild the router — all in one request (atomic DELETE).
    let settings = state.settings_manager.get().await;
    use crate::engine::llm_router::LlmSelection;
    if let LlmSelection::Local { id: active_id } = &settings.llm.selection {
        if active_id == &id {
            use crate::engine::app_settings::{LlmSettingsPatch, SettingsPatch};
            let patch = SettingsPatch {
                llm: Some(LlmSettingsPatch {
                    selection: Some(LlmSelection::Disabled),
                    ..Default::default()
                }),
                ..Default::default()
            };
            state.settings_manager.update(patch).await;
            *state.router.write().await = crate::build_router_from_settings(
                &state.settings_manager.get().await.llm,
                &state.engine_slot,
                state.remote_client_envseed.clone(),
            );
        }
    }

    Ok(Json(DeleteLlmResult { deleted: true }))
}

// ---------------------------------------------------------------------------
// OpenAI-compatible /v1/* routes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OpenAiChatRequest {
    pub model: Option<String>,
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub max_tokens: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub response_format: Option<serde_json::Value>,
}

#[derive(Deserialize, Clone)]
pub struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
pub struct OpenAiChatResponse {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<OpenAiChoice>,
    pub usage: OpenAiUsage,
}

#[derive(Serialize)]
pub struct OpenAiChoice {
    pub index: u32,
    pub message: OpenAiResponseMessage,
    pub finish_reason: &'static str,
}

#[derive(Serialize)]
pub struct OpenAiResponseMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Serialize)]
pub struct OpenAiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Serialize)]
pub struct OpenAiErrorEnvelope {
    pub error: OpenAiErrorBody,
}

#[derive(Serialize)]
pub struct OpenAiErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: &'static str,
}

#[derive(Serialize)]
pub struct OpenAiModelList {
    pub object: &'static str,
    pub data: Vec<OpenAiModelEntry>,
}

#[derive(Serialize)]
pub struct OpenAiModelEntry {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub owned_by: &'static str,
}

/// GET /v1/models — lists only the currently-loaded model.
pub async fn openai_list_models(State(state): State<AppState>) -> Json<OpenAiModelList> {
    let loaded = state.engine_slot.loaded_id().await;
    let data = match loaded {
        Some(id) => vec![OpenAiModelEntry {
            id,
            object: "model",
            created: 0,
            owned_by: "actio-local",
        }],
        None => vec![],
    };
    Json(OpenAiModelList {
        object: "list",
        data,
    })
}

/// POST /v1/chat/completions — OpenAI-compat completion against the local model.
/// Triggers lazy cold-start if a model is selected but not loaded.
pub async fn openai_chat_completions(
    State(state): State<AppState>,
    Json(req): Json<OpenAiChatRequest>,
) -> axum::response::Response {
    use crate::engine::llm_prompt::ChatMessage;
    use crate::engine::llm_router::LlmSelection;
    use crate::engine::local_llm_engine::{EnginePriority, GenerationParams};
    use axum::response::IntoResponse;

    // Determine the target model: check loaded engine first, then fall
    // back to the configured selection for lazy cold-start.
    let loaded_id = match state.engine_slot.loaded_id().await {
        Some(id) => id,
        None => {
            let settings = state.settings_manager.get().await;
            match &settings.llm.selection {
                LlmSelection::Local { id } => match state.engine_slot.get_or_load(id).await {
                    Ok(engine) => engine.loaded_id().to_string(),
                    Err(e) => {
                        return (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(OpenAiErrorEnvelope {
                                error: OpenAiErrorBody {
                                    message: format!("failed to load local model: {e}"),
                                    kind: "engine_load_failed",
                                },
                            }),
                        )
                            .into_response();
                    }
                },
                _ => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(OpenAiErrorEnvelope {
                            error: OpenAiErrorBody {
                                message: "no local model selected — configure one in Actio Settings → Language Models".into(),
                                kind: "no_model_loaded",
                            },
                        }),
                    )
                        .into_response();
                }
            }
        }
    };

    if let Some(requested) = req.model.as_deref() {
        if requested != loaded_id {
            return (
                StatusCode::NOT_FOUND,
                Json(OpenAiErrorEnvelope {
                    error: OpenAiErrorBody {
                        message: format!(
                            "model '{requested}' is not loaded; loaded model is '{loaded_id}'"
                        ),
                        kind: "model_not_found",
                    },
                }),
            )
                .into_response();
        }
    }

    let messages: Vec<ChatMessage> = req
        .messages
        .into_iter()
        .map(|m| ChatMessage {
            role: m.role,
            content: m.content,
        })
        .collect();

    let json_mode = req
        .response_format
        .as_ref()
        .and_then(|v| v.get("type"))
        .and_then(|t| t.as_str())
        == Some("json_object");

    let params = GenerationParams {
        max_tokens: req.max_tokens.unwrap_or(2000),
        temperature: req.temperature.unwrap_or(0.7),
        json_mode,
        thinking_budget: None,
    };

    let engine = match state.engine_slot.get_or_load(&loaded_id).await {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(OpenAiErrorEnvelope {
                    error: OpenAiErrorBody {
                        message: e.to_string(),
                        kind: "engine_load_failed",
                    },
                }),
            )
                .into_response();
        }
    };

    let is_stream = req.stream.unwrap_or(false);

    match engine
        .chat_completion(messages, params, EnginePriority::External)
        .await
    {
        Ok(content) => {
            let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            if is_stream {
                // Wrap as SSE: one chunk with the full content, then [DONE].
                let chunk = serde_json::json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": loaded_id,
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant", "content": content },
                        "finish_reason": null
                    }]
                });
                let done_chunk = serde_json::json!({
                    "id": id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": loaded_id,
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": "stop"
                    }]
                });
                let body = format!(
                    "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    serde_json::to_string(&chunk).unwrap(),
                    serde_json::to_string(&done_chunk).unwrap(),
                );
                axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .body(axum::body::Body::from(body))
                    .unwrap()
                    .into_response()
            } else {
                let resp = OpenAiChatResponse {
                    id,
                    object: "chat.completion",
                    created,
                    model: loaded_id.clone(),
                    choices: vec![OpenAiChoice {
                        index: 0,
                        message: OpenAiResponseMessage {
                            role: "assistant",
                            content,
                        },
                        finish_reason: "stop",
                    }],
                    usage: OpenAiUsage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        total_tokens: 0,
                    },
                };
                (StatusCode::OK, Json(resp)).into_response()
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(OpenAiErrorEnvelope {
                error: OpenAiErrorBody {
                    message: e.to_string(),
                    kind: "inference_failed",
                },
            }),
        )
            .into_response(),
    }
}
