use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::AppState;
use crate::repository::{session, speaker, transcript, todo};
use crate::domain::types::{AudioSession, Speaker, TodoItem, Transcript};

#[derive(Serialize, ToSchema)]
pub struct TodoListResponse {
    pub todos: Vec<TodoItem>,
    pub generated: bool,
}
use crate::engine::todo_generator;

#[derive(Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    pub tenant_id: Option<Uuid>,
    pub source_type: Option<String>,
    pub mode: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct SessionResponse {
    pub id: Uuid,
    pub started_at: String,
}

#[utoipa::path(
    post,
    path = "/sessions",
    request_body = CreateSessionRequest,
    tag = "sessions",
    responses(
        (status = 201, description = "Session created", body = SessionResponse),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<SessionResponse>), AppApiError> {
    let tenant_id = req.tenant_id.unwrap_or(tenant_id_from_headers(&headers)?);
    let source_type = req.source_type.as_deref().unwrap_or("microphone");
    let mode = req.mode.as_deref().unwrap_or("realtime");
    let s = session::create_session(&state.pool, tenant_id, source_type, mode)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(SessionResponse {
            id: s.id,
            started_at: s.started_at.to_rfc3339(),
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/sessions/{id}",
    tag = "sessions",
    params(
        ("id" = Uuid, Path, description = "Session ID"),
    ),
    responses(
        (status = 200, description = "Session details", body = AudioSession),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<AudioSession>, AppApiError> {
    let s = session::get_session(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(s))
}

#[utoipa::path(
    post,
    path = "/sessions/{id}/end",
    tag = "sessions",
    params(
        ("id" = Uuid, Path, description = "Session ID"),
    ),
    responses(
        (status = 204, description = "Session ended"),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn end_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    session::end_session(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;

    // Fire-and-forget todo generation (90s timeout)
    if let Some(llm_client) = state.llm_client.clone() {
        let pool = state.pool.clone();
        let tenant_id = session::get_session(&state.pool, id)
            .await
            .map(|session| session.tenant_id)
            .map_err(|e| AppApiError(e.to_string()))?;
        tokio::spawn(async move {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                todo_generator::generate_session_todos(&pool, &llm_client, id, tenant_id),
            )
            .await;
            match result {
                Ok(Ok(())) => info!(session_id = %id, "Todo generation completed"),
                Ok(Err(e)) => warn!(session_id = %id, error = %e, "Todo generation failed"),
                Err(_) => warn!(session_id = %id, "Todo generation timed out after 90s"),
            }
        });
    } else {
        info!(session_id = %id, "Skipping todo generation because LLM is not configured");
    }

    Ok(StatusCode::NO_CONTENT)
}
// --- Transcripts ---

#[utoipa::path(
    get,
    path = "/sessions/{id}/transcripts",
    tag = "transcripts",
    params(
        ("id" = Uuid, Path, description = "Session ID"),
    ),
    responses(
        (status = 200, description = "List of transcripts", body = Vec<Transcript>),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn get_transcripts(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Transcript>>, AppApiError> {
    let transcripts = transcript::get_transcripts_for_session(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(transcripts))
}

// --- Todos ---

#[utoipa::path(
    get,
    path = "/sessions/{id}/todos",
    tag = "todos",
    params(
        ("id" = Uuid, Path, description = "Session ID"),
    ),
    responses(
        (status = 200, description = "List of todo items", body = TodoListResponse),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn get_todo_items(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TodoListResponse>, AppApiError> {
    let todos = todo::get_todos_for_session(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(TodoListResponse { todos, generated: true }))
}

// --- Speaker ---

#[derive(Deserialize, ToSchema)]
pub struct CreateSpeakerRequest {
    pub tenant_id: Option<Uuid>,
    pub display_name: String,
}

#[utoipa::path(
    post,
    path = "/speakers",
    request_body = CreateSpeakerRequest,
    tag = "speakers",
    responses(
        (status = 201, description = "Speaker created", body = Speaker),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn create_speaker(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSpeakerRequest>,
) -> Result<(StatusCode, Json<Speaker>), AppApiError> {
    let tenant_id = req.tenant_id.unwrap_or(tenant_id_from_headers(&headers)?);
    let s = speaker::create_speaker(&state.pool, &req.display_name, tenant_id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(s)))
}

#[utoipa::path(
    get,
    path = "/speakers",
    tag = "speakers",
    responses(
        (status = 200, description = "List of speakers", body = Vec<Speaker>),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn list_speakers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Speaker>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers)?;
    let speakers = speaker::list_speakers(&state.pool, tenant_id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(speakers))
}

// --- Error ---

#[derive(Debug, ToSchema)]
#[allow(dead_code)]
pub struct AppApiError(pub String);

impl axum::response::IntoResponse for AppApiError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!(error = %self.0, "Internal server error");
        (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
    }
}

fn tenant_id_from_headers(headers: &HeaderMap) -> Result<Uuid, AppApiError> {
    match headers.get("x-tenant-id") {
        Some(value) => value
            .to_str()
            .map_err(|e| AppApiError(e.to_string()))
            .and_then(|value| Uuid::parse_str(value).map_err(|e| AppApiError(e.to_string()))),
        None => Ok(Uuid::nil()),
    }
}
