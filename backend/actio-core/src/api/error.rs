//! Shared API error type. Closes ISSUES.md #67 — used to live in
//! `api/session.rs` where 7 sibling modules had to reach across to import
//! it; centralized here so `use crate::api::error::AppApiError;` is the
//! single import line.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use utoipa::ToSchema;

#[derive(Debug, ToSchema)]
#[allow(dead_code)]
pub enum AppApiError {
    Internal(String),
    BadRequest(String),
    Conflict(String),
}

impl IntoResponse for AppApiError {
    fn into_response(self) -> Response {
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
