use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::session::AppApiError;
use crate::domain::types::{NewReminder, PatchReminderRequest, Reminder, ReminderFilter};
use crate::repository::reminder as reminder_repo;
use crate::AppState;

fn tenant_id_from_headers(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Uuid::nil())
}

#[derive(Debug, Deserialize)]
pub struct ListRemindersQuery {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub label_id: Option<Uuid>,
    pub search: Option<String>,
    pub session_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_reminders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListRemindersQuery>,
) -> Result<Json<Vec<Reminder>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers);
    let filter = ReminderFilter {
        status: q.status,
        priority: q.priority,
        label_id: q.label_id,
        search: q.search,
        session_id: q.session_id,
        limit: q.limit.unwrap_or(50).min(200),
        offset: q.offset.unwrap_or(0),
    };
    let reminders = reminder_repo::list_reminders(&state.pool, tenant_id, &filter)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(reminders))
}

#[derive(Debug, Deserialize)]
pub struct CreateReminderRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub due_time: Option<chrono::DateTime<chrono::Utc>>,
    pub labels: Option<Vec<Uuid>>,
    pub session_id: Option<Uuid>,
}

pub async fn create_reminder(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateReminderRequest>,
) -> Result<(StatusCode, Json<Reminder>), AppApiError> {
    if req.title.is_none() && req.description.is_none() {
        return Err(AppApiError("title or description is required".into()));
    }
    let tenant_id = tenant_id_from_headers(&headers);
    let label_ids = req.labels.as_deref().unwrap_or(&[]);
    let new_reminder = NewReminder {
        session_id: req.session_id,
        tenant_id,
        speaker_id: None,
        assigned_to: None,
        title: req.title,
        description: req.description.unwrap_or_default(),
        priority: req.priority,
        due_time: req.due_time,
        transcript_excerpt: None,
        context: None,
        source_time: None,
    };
    let reminder = reminder_repo::create_reminder(&state.pool, &new_reminder, label_ids)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(reminder)))
}

pub async fn get_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Reminder>, AppApiError> {
    match reminder_repo::get_reminder(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?
    {
        Some(r) => Ok(Json(r)),
        None => Err(AppApiError("not found".into())),
    }
}

pub async fn patch_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<PatchReminderRequest>,
) -> Result<Json<Reminder>, AppApiError> {
    if let Some(ref s) = patch.status {
        if !["open", "completed", "archived"].contains(&s.as_str()) {
            return Err(AppApiError(format!("invalid status: {s}")));
        }
    }
    if let Some(ref p) = patch.priority {
        if !["high", "medium", "low"].contains(&p.as_str()) {
            return Err(AppApiError(format!("invalid priority: {p}")));
        }
    }
    match reminder_repo::patch_reminder(&state.pool, id, &patch)
        .await
        .map_err(|e| AppApiError(e.to_string()))?
    {
        Some(r) => Ok(Json(r)),
        None => Err(AppApiError("not found".into())),
    }
}

pub async fn delete_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = reminder_repo::delete_reminder(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError("not found".into()))
    }
}
