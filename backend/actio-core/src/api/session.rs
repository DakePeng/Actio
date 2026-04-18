use axum::extract::{Multipart, Path, Query, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::domain::types::{AudioSession, ListSessionsParams, Speaker, TodoItem, Transcript};
use crate::repository::{session, speaker, todo, transcript};
use crate::AppState;

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
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

    // Attempt to start inference pipeline if models are ready (graceful degradation)
    if let Some(model_paths) = state.model_manager.model_paths().await {
        let mut pipeline = state.inference_pipeline.lock().await;
        if !pipeline.is_running() {
            let settings = state.settings_manager.get().await;
            let asr_model = settings.audio.asr_model.as_deref();
            if let Err(e) = pipeline.start_session(
                s.id.parse::<Uuid>().unwrap_or_default(),
                &model_paths,
                state.aggregator.clone(),
                None,
                asr_model,
            ) {
                warn!(session_id = %s.id, error = %e, "Failed to start inference pipeline — CRUD-only mode");
            }
        }
    } else {
        info!(session_id = %s.id, "Models not ready — session running in CRUD-only mode");
    }

    Ok((
        StatusCode::CREATED,
        Json(SessionResponse {
            id: s.id.parse::<Uuid>().unwrap_or_default(),
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
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
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
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

    // Stop the inference pipeline if running
    state.inference_pipeline.lock().await.stop();

    // Fire-and-forget todo generation (90s timeout)
    {
        let router = state.router.clone();
        let pool = state.pool.clone();
        let tenant_id = session::get_session(&state.pool, id)
            .await
            .map(|session| session.tenant_id.parse::<Uuid>().unwrap_or_default())
            .map_err(|e| AppApiError::Internal(e.to_string()))?;
        tokio::spawn(async move {
            let router_guard = router.read().await;
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                todo_generator::generate_session_todos(&pool, &*router_guard, id, tenant_id),
            )
            .await;
            match result {
                Ok(Ok(())) => info!(session_id = %id, "Todo generation completed"),
                Ok(Err(e)) => warn!(session_id = %id, error = %e, "Todo generation failed"),
                Err(_) => warn!(session_id = %id, "Todo generation timed out after 90s"),
            }
        });
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
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
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
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(TodoListResponse {
        todos,
        generated: true,
    }))
}

// --- Speaker ---

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSpeakerRequest {
    pub display_name: String,
    #[serde(default = "default_color")]
    pub color: String,
}

fn default_color() -> String {
    "#64B5F6".into()
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
    let tenant_id = tenant_id_from_headers(&headers)?;
    let s = speaker::create_speaker(&state.pool, &req.display_name, &req.color, tenant_id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
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
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(speakers))
}

// --- Session listing ---

#[utoipa::path(
    get,
    path = "/sessions",
    tag = "sessions",
    responses(
        (status = 200, description = "List of sessions", body = Vec<AudioSession>),
    ),
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<Vec<AudioSession>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers)?;
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);
    let sessions = session::list_sessions(&state.pool, tenant_id, limit, offset)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(sessions))
}

// --- Speaker mutations ---

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateSpeakerRequest {
    pub display_name: Option<String>,
    pub color: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/speakers/{id}",
    tag = "speakers",
    params(("id" = Uuid, Path, description = "Speaker ID")),
    responses(
        (status = 200, description = "Updated speaker", body = Speaker),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn update_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSpeakerRequest>,
) -> Result<Json<Speaker>, AppApiError> {
    match speaker::update_speaker(
        &state.pool,
        id,
        req.display_name.as_deref(),
        req.color.as_deref(),
    )
    .await
    {
        Ok(Some(s)) => Ok(Json(s)),
        Ok(None) => Err(AppApiError::Internal("speaker not found".into())),
        Err(e) => Err(AppApiError::Internal(e.to_string())),
    }
}

#[utoipa::path(
    delete,
    path = "/speakers/{id}",
    tag = "speakers",
    params(("id" = Uuid, Path, description = "Speaker ID")),
    responses(
        (status = 204, description = "Speaker deleted"),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn delete_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = speaker::delete_speaker_with_segment_cleanup(&state.pool, id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError::Internal("speaker not found".into()))
    }
}

// --- Speaker enrollment ---

#[derive(Debug, Serialize, ToSchema)]
pub struct EnrolledEmbedding {
    pub id: String,
    pub duration_ms: f64,
    pub quality_score: f64,
    pub is_primary: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EnrollResponse {
    pub speaker_id: String,
    pub embeddings: Vec<EnrolledEmbedding>,
    pub warnings: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/speakers/{id}/enroll",
    tag = "speakers",
    params(("id" = Uuid, Path, description = "Speaker ID")),
    responses(
        (status = 201, description = "Voiceprint enrolled", body = EnrollResponse),
        (status = 400, description = "No valid clips", body = AppApiError),
        (status = 409, description = "Embedding model missing", body = AppApiError),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn enroll_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<EnrollResponse>), AppApiError> {
    // Resolve the embedding model; 409 if missing.
    let model_paths = state
        .model_manager
        .model_paths()
        .await
        .ok_or_else(|| AppApiError::Conflict("embedding_model_missing".into()))?;
    let model_path = model_paths
        .speaker_embedding
        .clone()
        .ok_or_else(|| AppApiError::Conflict("embedding_model_missing".into()))?;

    // Collect clip bytes from multipart parts named clip_*
    let mut raw_clips: Vec<(String, Vec<u8>)> = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppApiError::BadRequest(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();
        if !name.starts_with("clip_") {
            continue;
        }
        let data = field
            .bytes()
            .await
            .map_err(|e| AppApiError::BadRequest(e.to_string()))?
            .to_vec();
        raw_clips.push((name, data));
    }

    if raw_clips.is_empty() {
        return Err(AppApiError::BadRequest(
            "no_valid_clips: empty upload".into(),
        ));
    }

    // Decode + extract embeddings (NO DB writes yet).
    struct Prepared {
        embedding: Vec<f32>,
        duration_ms: f64,
        quality: f64,
    }
    let mut prepared: Vec<Prepared> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for (name, bytes) in raw_clips {
        let (samples, duration_ms) = match crate::engine::wav::decode_to_mono_16k(&bytes) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(format!("{name}: failed to decode ({e})"));
                continue;
            }
        };

        if duration_ms < 3_000.0 {
            warnings.push(format!(
                "{name}: skipped — duration {:.1}s < 3s minimum",
                duration_ms / 1000.0
            ));
            continue;
        }
        if duration_ms > 30_000.0 {
            warnings.push(format!(
                "{name}: skipped — duration {:.1}s > 30s maximum",
                duration_ms / 1000.0
            ));
            continue;
        }

        let emb = match crate::engine::diarization::extract_embedding(&model_path, &samples).await {
            Ok(e) => e,
            Err(e) => {
                warnings.push(format!("{name}: extraction failed ({e})"));
                continue;
            }
        };

        prepared.push(Prepared {
            embedding: emb.values,
            duration_ms,
            quality: crate::engine::audio_quality::score(&samples) as f64,
        });
    }

    if prepared.is_empty() {
        return Err(AppApiError::BadRequest(format!(
            "no_valid_clips: {}",
            warnings.join("; ")
        )));
    }

    // Transactional delete-then-insert (mode=replace is the v1 default).
    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

    sqlx::query("DELETE FROM speaker_embeddings WHERE speaker_id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

    let mut inserted: Vec<EnrolledEmbedding> = Vec::new();
    for (i, p) in prepared.iter().enumerate() {
        let is_primary = i == 0;
        let new_id = Uuid::new_v4().to_string();
        let blob: &[u8] = bytemuck::cast_slice(&p.embedding);
        sqlx::query(
            "INSERT INTO speaker_embeddings \
               (id, speaker_id, embedding, duration_ms, quality_score, is_primary, embedding_dimension) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(&new_id)
        .bind(id.to_string())
        .bind(blob)
        .bind(p.duration_ms)
        .bind(p.quality)
        .bind(is_primary as i64)
        .bind(p.embedding.len() as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

        inserted.push(EnrolledEmbedding {
            id: new_id,
            duration_ms: p.duration_ms,
            quality_score: p.quality,
            is_primary,
        });
    }

    tx.commit()
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(EnrollResponse {
            speaker_id: id.to_string(),
            embeddings: inserted,
            warnings,
        }),
    ))
}

// --- Error ---

#[derive(Debug, ToSchema)]
#[allow(dead_code)]
pub enum AppApiError {
    Internal(String),
    BadRequest(String),
    Conflict(String),
}

impl axum::response::IntoResponse for AppApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            Self::Internal(msg) => {
                tracing::error!(error = %msg, "Internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": msg})),
                )
                    .into_response()
            }
            Self::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response(),
            Self::Conflict(msg) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({"error": msg})),
            )
                .into_response(),
        }
    }
}

fn tenant_id_from_headers(headers: &HeaderMap) -> Result<Uuid, AppApiError> {
    match headers.get("x-tenant-id") {
        Some(value) => value
            .to_str()
            .map_err(|e| AppApiError::Internal(e.to_string()))
            .and_then(|value| Uuid::parse_str(value).map_err(|e| AppApiError::Internal(e.to_string()))),
        None => Ok(Uuid::nil()),
    }
}
