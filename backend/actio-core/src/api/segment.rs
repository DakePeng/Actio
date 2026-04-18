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

/// One cluster of voiceprint-candidate segments the user could be asked
/// about. The `audio_ref` is the filename of the longest-duration member
/// (relative to `AppState.clips_dir`) and serves as the audio preview.
#[derive(Debug, Serialize, ToSchema)]
pub struct VoiceprintCandidateResponse {
    /// Stable id derived from the representative segment. Clients echo it
    /// back when confirming/dismissing the prompt.
    pub candidate_id: String,
    pub representative_segment_id: String,
    pub audio_ref: String,
    pub session_id: String,
    /// Number of segments in this cluster.
    pub occurrences: usize,
    /// Sum of segment durations in milliseconds.
    pub total_duration_ms: i64,
    pub earliest_ms: i64,
    pub latest_ms: i64,
    /// All member segment ids, newest-first.
    pub member_segment_ids: Vec<String>,
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

#[derive(Debug, Deserialize)]
pub struct ListCandidatesQuery {
    pub session_id: Option<Uuid>,
    #[serde(default = "default_candidate_limit")]
    pub limit: i64,
}
fn default_candidate_limit() -> i64 {
    200
}

/// Phase B: cluster retained voiceprint-candidate segments and return one
/// entry per distinct unknown voice. Clusters are ordered by `occurrences`
/// descending so the UI can prompt about the most-heard voice first.
///
/// This is computed on-demand rather than materialised — input volume is
/// small (retention caps it to a few days' worth of quality-gated segments)
/// and clusters would otherwise need maintenance as the underlying rows
/// get added or pruned.
#[utoipa::path(
    get,
    path = "/candidates",
    tag = "segments",
    responses(
        (status = 200, description = "Clustered voiceprint candidates", body = Vec<VoiceprintCandidateResponse>),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn list_candidates(
    State(state): State<AppState>,
    Query(params): Query<ListCandidatesQuery>,
) -> Result<Json<Vec<VoiceprintCandidateResponse>>, AppApiError> {
    let rows = crate::repository::segment::list_retained_candidates(
        &state.pool,
        params.session_id,
        params.limit,
    )
    .await
    .map_err(|e| AppApiError::Internal(e.to_string()))?;

    // Decode BLOBs → f32 embeddings, skip any that the align-check rejects
    // so a corrupt row can't poison the whole request.
    let segments: Vec<crate::engine::voiceprint_clustering::CandidateSegment> = rows
        .into_iter()
        .filter_map(|r| {
            let emb = bytemuck::try_cast_slice::<u8, f32>(&r.embedding)
                .ok()
                .map(|s| s.to_vec())?;
            Some(crate::engine::voiceprint_clustering::CandidateSegment {
                id: r.id,
                session_id: r.session_id,
                audio_ref: r.audio_ref,
                start_ms: r.start_ms,
                end_ms: r.end_ms,
                embedding: emb,
            })
        })
        .collect();

    let clusters = crate::engine::voiceprint_clustering::cluster_candidates(
        segments,
        crate::engine::voiceprint_clustering::DEFAULT_CLUSTER_THRESHOLD,
    );

    let out: Vec<VoiceprintCandidateResponse> = clusters
        .into_iter()
        .map(|c| VoiceprintCandidateResponse {
            candidate_id: format!("cand_{}", c.representative.id),
            representative_segment_id: c.representative.id.clone(),
            audio_ref: c.representative.audio_ref.clone(),
            session_id: c.representative.session_id.clone(),
            occurrences: c.occurrences,
            total_duration_ms: c.total_duration_ms,
            earliest_ms: c.earliest_ms,
            latest_ms: c.latest_ms,
            member_segment_ids: c.member_ids,
        })
        .collect();

    Ok(Json(out))
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
