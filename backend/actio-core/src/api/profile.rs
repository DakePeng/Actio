//! GET / PUT /profile — tenant identity used by the action-item extractor.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::domain::types::TenantProfile;
use crate::repository::tenant_profile as repo;
use crate::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateProfileRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub aliases: Option<Vec<String>>,
    #[serde(default)]
    pub bio: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ProfileResponse {
    pub tenant_id: Uuid,
    pub display_name: Option<String>,
    pub aliases: Vec<String>,
    pub bio: Option<String>,
}

impl From<TenantProfile> for ProfileResponse {
    fn from(p: TenantProfile) -> Self {
        Self {
            tenant_id: p.tenant_id,
            display_name: p.display_name,
            aliases: p.aliases,
            bio: p.bio,
        }
    }
}

fn tenant_id_from_headers(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Uuid::nil())
}

#[utoipa::path(
    get,
    path = "/profile",
    responses(
        (status = 200, description = "Tenant profile", body = ProfileResponse),
        (status = 404, description = "No profile set"),
    )
)]
pub async fn get_profile(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let tenant_id = tenant_id_from_headers(&headers);
    match repo::get_for_tenant(&state.pool, tenant_id).await {
        Ok(Some(p)) => Json(ProfileResponse::from(p)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "get_profile failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[utoipa::path(
    put,
    path = "/profile",
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Updated profile", body = ProfileResponse),
    )
)]
pub async fn put_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UpdateProfileRequest>,
) -> impl IntoResponse {
    let tenant_id = tenant_id_from_headers(&headers);
    let existing = repo::get_for_tenant(&state.pool, tenant_id)
        .await
        .ok()
        .flatten();
    let merged = TenantProfile {
        tenant_id,
        display_name: req
            .display_name
            .or_else(|| existing.as_ref().and_then(|p| p.display_name.clone())),
        aliases: req.aliases.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|p| p.aliases.clone())
                .unwrap_or_default()
        }),
        bio: req.bio.or_else(|| existing.and_then(|p| p.bio)),
    };
    if let Err(e) = repo::upsert(&state.pool, &merged).await {
        tracing::warn!(error = %e, "put_profile upsert failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Json(ProfileResponse::from(merged)).into_response()
}
