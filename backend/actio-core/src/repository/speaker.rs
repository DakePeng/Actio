use crate::domain::types::Speaker;
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn create_speaker(
    pool: &SqlitePool,
    display_name: &str,
    tenant_id: Uuid,
) -> Result<Speaker, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query_as::<_, Speaker>(
        "INSERT INTO speakers (id, display_name, tenant_id) VALUES (?1, ?2, ?3) RETURNING *",
    )
    .bind(&id)
    .bind(display_name)
    .bind(tenant_id.to_string())
    .fetch_one(pool)
    .await
}

pub async fn get_speaker(pool: &SqlitePool, id: Uuid) -> Result<Speaker, sqlx::Error> {
    sqlx::query_as::<_, Speaker>("SELECT * FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .fetch_one(pool)
        .await
}

pub async fn list_speakers(
    pool: &SqlitePool,
    tenant_id: Uuid,
) -> Result<Vec<Speaker>, sqlx::Error> {
    sqlx::query_as::<_, Speaker>(
        "SELECT * FROM speakers WHERE tenant_id = ?1 AND status = 'active' ORDER BY created_at DESC"
    )
    .bind(tenant_id.to_string())
    .fetch_all(pool)
    .await
}

pub async fn update_speaker(
    pool: &SqlitePool,
    id: Uuid,
    display_name: &str,
) -> Result<Option<Speaker>, sqlx::Error> {
    sqlx::query_as::<_, Speaker>("UPDATE speakers SET display_name = ?1 WHERE id = ?2 RETURNING *")
        .bind(display_name)
        .bind(id.to_string())
        .fetch_optional(pool)
        .await
}

pub async fn soft_delete_speaker(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("UPDATE speakers SET status = 'inactive' WHERE id = ?1 AND status = 'active'")
            .bind(id.to_string())
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}
