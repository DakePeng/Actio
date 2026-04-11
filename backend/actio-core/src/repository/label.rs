use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::types::{CreateLabelRequest, Label, PatchLabelRequest};

pub async fn list_labels(pool: &SqlitePool, tenant_id: Uuid) -> Result<Vec<Label>, sqlx::Error> {
    sqlx::query_as::<_, Label>(
        "SELECT * FROM labels WHERE tenant_id = ?1 ORDER BY name ASC",
    )
    .bind(tenant_id.to_string())
    .fetch_all(pool)
    .await
}

pub async fn create_label(
    pool: &SqlitePool,
    tenant_id: Uuid,
    req: &CreateLabelRequest,
) -> Result<Label, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query_as::<_, Label>(
        "INSERT INTO labels (id, tenant_id, name, color, bg_color) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING *",
    )
    .bind(&id)
    .bind(tenant_id.to_string())
    .bind(&req.name)
    .bind(&req.color)
    .bind(&req.bg_color)
    .fetch_one(pool)
    .await
}

pub async fn patch_label(
    pool: &SqlitePool,
    id: Uuid,
    req: &PatchLabelRequest,
) -> Result<Option<Label>, sqlx::Error> {
    sqlx::query_as::<_, Label>(
        r#"UPDATE labels SET
               name     = COALESCE(?1, name),
               color    = COALESCE(?2, color),
               bg_color = COALESCE(?3, bg_color)
           WHERE id = ?4
           RETURNING *"#,
    )
    .bind(&req.name)
    .bind(&req.color)
    .bind(&req.bg_color)
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
}

pub async fn delete_label(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM labels WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
