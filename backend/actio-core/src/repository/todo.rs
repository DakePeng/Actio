use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::types::TodoItem;

/// Get all reminders for a session — used by the backward-compat alias route.
pub async fn get_todos_for_session(
    pool: &SqlitePool,
    session_id: Uuid,
) -> Result<Vec<TodoItem>, sqlx::Error> {
    sqlx::query_as::<_, TodoItem>(
        "SELECT * FROM reminders WHERE session_id = ?1 ORDER BY created_at ASC",
    )
    .bind(session_id.to_string())
    .fetch_all(pool)
    .await
}
