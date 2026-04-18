use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::api::session::{tenant_id_from_headers, AppApiError};
use crate::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct UnknownSegmentResponse {
    pub segment_id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListUnknownsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}
fn default_limit() -> i64 {
    50
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AssignSegmentRequest {
    pub speaker_id: Option<Uuid>,
    pub new_speaker: Option<NewSpeakerSpec>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct NewSpeakerSpec {
    pub display_name: String,
    #[serde(default = "default_color")]
    pub color: String,
}
fn default_color() -> String {
    "#64B5F6".into()
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AssignSegmentResponse {
    pub segment_id: String,
    pub speaker_id: String,
}

fn to_response(r: crate::repository::segment::UnknownSegmentRow) -> UnknownSegmentResponse {
    UnknownSegmentResponse {
        segment_id: r.id,
        session_id: r.session_id,
        start_ms: r.start_ms,
        end_ms: r.end_ms,
    }
}

#[utoipa::path(
    get,
    path = "/sessions/{id}/unknowns",
    tag = "segments",
    params(("id" = Uuid, Path, description = "Session ID")),
    responses(
        (status = 200, description = "Unknown segments in the session", body = Vec<UnknownSegmentResponse>),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn list_session_unknowns(
    State(state): State<AppState>,
    Path(session_id): Path<Uuid>,
    Query(params): Query<ListUnknownsQuery>,
) -> Result<Json<Vec<UnknownSegmentResponse>>, AppApiError> {
    let rows = crate::repository::segment::list_unknown_segments(
        &state.pool,
        Some(session_id),
        params.limit,
    )
    .await
    .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}

#[utoipa::path(
    get,
    path = "/unknowns",
    tag = "segments",
    responses(
        (status = 200, description = "Unknown segments across sessions", body = Vec<UnknownSegmentResponse>),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn list_unknowns(
    State(state): State<AppState>,
    Query(params): Query<ListUnknownsQuery>,
) -> Result<Json<Vec<UnknownSegmentResponse>>, AppApiError> {
    let rows = crate::repository::segment::list_unknown_segments(&state.pool, None, params.limit)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(rows.into_iter().map(to_response).collect()))
}

/// Assign a speaker to an unknown segment. This is label-only: the
/// segment's `speaker_id` is updated so past transcripts display the
/// speaker's name, but the segment audio is NOT promoted into the
/// speaker's voiceprint collection. Voiceprints are curated exclusively
/// through `POST /speakers/{id}/enroll`, which captures deliberately-read
/// prompts of known quality. Opportunistic transcription segments are too
/// noisy and uncurated to be trusted as permanent identification data.
#[utoipa::path(
    post,
    path = "/segments/{id}/assign",
    tag = "segments",
    params(("id" = Uuid, Path, description = "Segment ID")),
    request_body = AssignSegmentRequest,
    responses(
        (status = 200, description = "Segment assigned", body = AssignSegmentResponse),
        (status = 400, description = "Bad request", body = AppApiError),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn assign_segment(
    State(state): State<AppState>,
    Path(segment_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<AssignSegmentRequest>,
) -> Result<Json<AssignSegmentResponse>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers)?;
    let target_speaker_id = match (body.speaker_id, body.new_speaker) {
        (Some(id), _) => id,
        (None, Some(spec)) => {
            let s = crate::repository::speaker::create_speaker(
                &state.pool,
                &spec.display_name,
                &spec.color,
                tenant_id,
            )
            .await
            .map_err(|e| AppApiError::Internal(e.to_string()))?;
            Uuid::parse_str(&s.id).map_err(|e| AppApiError::Internal(e.to_string()))?
        }
        _ => {
            return Err(AppApiError::BadRequest(
                "speaker_id or new_speaker required".into(),
            ))
        }
    };

    let updated =
        crate::repository::segment::assign_speaker(&state.pool, segment_id, target_speaker_id)
            .await
            .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if !updated {
        return Err(AppApiError::BadRequest("segment not found".into()));
    }

    Ok(Json(AssignSegmentResponse {
        segment_id: segment_id.to_string(),
        speaker_id: target_speaker_id.to_string(),
    }))
}

#[utoipa::path(
    post,
    path = "/segments/{id}/unassign",
    tag = "segments",
    params(("id" = Uuid, Path, description = "Segment ID")),
    responses(
        (status = 204, description = "Segment unassigned"),
        (status = 400, description = "Segment not found", body = AppApiError),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn unassign_segment(
    State(state): State<AppState>,
    Path(segment_id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let ok = crate::repository::segment::unassign_speaker(&state.pool, segment_id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if ok {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError::BadRequest("segment not found".into()))
    }
}
