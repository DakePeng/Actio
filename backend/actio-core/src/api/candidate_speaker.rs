//! Candidate Speakers panel — HTTP routes for managing provisional
//! speakers auto-created by the batch processor's clustering pass.
//!
//! Three operations:
//!   GET    /candidate-speakers              — list all provisionals
//!   POST   /candidate-speakers/:id/promote  — rename + flip kind=enrolled
//!   DELETE /candidate-speakers/:id          — hard-delete (segments → NULL)
//!
//! Promotion makes the row indistinguishable from any user-enrolled
//! speaker; dismissal severs all segment links and removes the row.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::api::session::AppApiError;
use crate::repository::speaker as speaker_repo;
use crate::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct CandidateSpeaker {
    pub id: Uuid,
    pub display_name: String,
    pub color: String,
    pub last_matched_at: Option<String>,
}

#[utoipa::path(
    get,
    path = "/candidate-speakers",
    tag = "candidate-speakers",
    responses(
        (status = 200, description = "Provisional speakers awaiting promote/dismiss", body = Vec<CandidateSpeaker>),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn list_candidates(
    State(state): State<AppState>,
) -> Result<Json<Vec<CandidateSpeaker>>, AppApiError> {
    let rows = speaker_repo::list_provisional(&state.pool)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    Ok(Json(
        rows.into_iter()
            .map(|r| CandidateSpeaker {
                id: Uuid::parse_str(&r.id).unwrap_or_default(),
                display_name: r.display_name,
                color: r.color,
                last_matched_at: r.provisional_last_matched_at,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PromoteBody {
    /// New display name for the speaker. If null/empty, the auto-generated
    /// "Unknown YYYY-MM-DD HH:MM" name is preserved.
    pub display_name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/candidate-speakers/{id}/promote",
    tag = "candidate-speakers",
    params(("id" = Uuid, Path, description = "Provisional speaker ID")),
    request_body = PromoteBody,
    responses(
        (status = 204, description = "Promoted to enrolled"),
        (status = 400, description = "No provisional row with that id", body = AppApiError),
    ),
)]
pub async fn promote(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<PromoteBody>,
) -> Result<StatusCode, AppApiError> {
    let new_name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let promoted = speaker_repo::promote_provisional(&state.pool, id, new_name)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if promoted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError::BadRequest(
            "no provisional speaker with that id".into(),
        ))
    }
}

#[utoipa::path(
    delete,
    path = "/candidate-speakers/{id}",
    tag = "candidate-speakers",
    params(("id" = Uuid, Path, description = "Provisional speaker ID")),
    responses(
        (status = 204, description = "Dismissed (row deleted; segments unlinked)"),
        (status = 400, description = "No provisional row with that id", body = AppApiError),
    ),
)]
pub async fn dismiss(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let dismissed = speaker_repo::dismiss_provisional(&state.pool, id)
        .await
        .map_err(|e| AppApiError::Internal(e.to_string()))?;
    if dismissed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError::BadRequest(
            "no provisional speaker with that id".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::speaker::insert_provisional;

    use crate::testing::fresh_pool;

    #[tokio::test]
    async fn promote_renames_and_clears_provisional_state() {
        let pool = fresh_pool().await;
        let id = Uuid::new_v4();
        insert_provisional(&pool, id, Uuid::nil(), "Unknown 2026-04-25", "#aaa")
            .await
            .unwrap();

        let promoted = speaker_repo::promote_provisional(&pool, id, Some("Bob"))
            .await
            .unwrap();
        assert!(promoted);

        let row: (String, String, Option<String>) = sqlx::query_as(
            "SELECT display_name, kind, provisional_last_matched_at FROM speakers WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "Bob");
        assert_eq!(row.1, "enrolled");
        assert!(row.2.is_none());
    }

    #[tokio::test]
    async fn promote_without_new_name_keeps_existing() {
        let pool = fresh_pool().await;
        let id = Uuid::new_v4();
        insert_provisional(&pool, id, Uuid::nil(), "Unknown 2026-04-25", "#aaa")
            .await
            .unwrap();

        speaker_repo::promote_provisional(&pool, id, None)
            .await
            .unwrap();

        let name: (String,) = sqlx::query_as("SELECT display_name FROM speakers WHERE id = ?1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(name.0, "Unknown 2026-04-25");
    }

    #[tokio::test]
    async fn dismiss_removes_row() {
        let pool = fresh_pool().await;
        let id = Uuid::new_v4();
        insert_provisional(&pool, id, Uuid::nil(), "Unknown", "#aaa")
            .await
            .unwrap();
        let removed = speaker_repo::dismiss_provisional(&pool, id).await.unwrap();
        assert!(removed);
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speakers WHERE id = ?1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn promote_idempotent_against_already_enrolled() {
        let pool = fresh_pool().await;
        let id = Uuid::new_v4();
        // Create as enrolled directly. The promote call should be a no-op
        // (returns false) since the WHERE kind='provisional' guard blocks it.
        sqlx::query(
            "INSERT INTO speakers (id, tenant_id, display_name, color, status, kind) \
             VALUES (?1, ?2, 'Alice', '#bbb', 'active', 'enrolled')",
        )
        .bind(id.to_string())
        .bind(Uuid::nil().to_string())
        .execute(&pool)
        .await
        .unwrap();

        let promoted = speaker_repo::promote_provisional(&pool, id, Some("Bob"))
            .await
            .unwrap();
        assert!(!promoted, "promote of already-enrolled must be a no-op");
        let name: (String,) = sqlx::query_as("SELECT display_name FROM speakers WHERE id = ?1")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(name.0, "Alice");
    }
}
