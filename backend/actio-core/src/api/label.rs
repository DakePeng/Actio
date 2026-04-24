use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use uuid::Uuid;

use crate::api::session::AppApiError;
use crate::domain::types::{CreateLabelRequest, Label, PatchLabelRequest};
use crate::repository::label as label_repo;
use crate::AppState;

fn tenant_id_from_headers(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Uuid::nil())
}

pub async fn list_labels(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Label>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers);
    let labels = label_repo::list_labels(&state.pool, tenant_id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(labels))
}

pub async fn create_label(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateLabelRequest>,
) -> Result<(StatusCode, Json<Label>), AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers);
    match label_repo::create_label(&state.pool, tenant_id, &req).await {
        Ok(label) => Ok((StatusCode::CREATED, Json(label))),
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("labels_tenant_id_name_key") => {
            Err(AppApiError::Internal(format!(
                "label '{}' already exists",
                req.name
            )))
        }
        Err(e) => Err(AppApiError::Internal(e.to_string())),
    }
}

pub async fn patch_label(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchLabelRequest>,
) -> Result<Json<Label>, AppApiError> {
    match label_repo::patch_label(&state.pool, id, &req)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?
    {
        Some(l) => Ok(Json(l)),
        None => Err(AppApiError::Internal("not found".into())),
    }
}

pub async fn delete_label(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = label_repo::delete_label(&state.pool, id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError::Internal("not found".into()))
    }
}
