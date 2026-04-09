use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::types::TodoItem;
use crate::domain::types::NewTodo;

/// Check if any reminders already exist for a session (idempotency guard).
#[allow(dead_code)]
pub async fn has_todos(pool: &PgPool, session_id: Uuid) -> Result<bool, sqlx::Error> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM reminders WHERE session_id = $1)"
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Batch insert reminders — used by the backward-compat alias route.
#[allow(dead_code)]
pub async fn create_todos(
    pool: &PgPool,
    items: &[NewTodo],
) -> Result<Vec<TodoItem>, sqlx::Error> {
    if items.is_empty() {
        return Ok(vec![]);
    }

    let mut results = Vec::with_capacity(items.len());

    for item in items {
        let row: Option<TodoItem> = sqlx::query_as(
            "INSERT INTO reminders (session_id, speaker_id, assigned_to, description, priority) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (session_id, description) DO NOTHING \
             RETURNING *"
        )
        .bind(item.session_id)
        .bind(item.speaker_id)
        .bind(&item.assigned_to)
        .bind(&item.description)
        .bind(&item.priority)
        .fetch_optional(pool)
        .await?;

        if let Some(todo) = row {
            results.push(todo);
        }
    }

    Ok(results)
}

/// Get all reminders for a session — used by the backward-compat alias route.
pub async fn get_todos_for_session(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Vec<TodoItem>, sqlx::Error> {
    sqlx::query_as::<_, TodoItem>(
        "SELECT * FROM reminders WHERE session_id = $1 ORDER BY created_at ASC"
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
}
