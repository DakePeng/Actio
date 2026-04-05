use crate::domain::types::Speaker;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_speaker(
    pool: &PgPool,
    display_name: &str,
    tenant_id: Uuid,
) -> Result<Speaker, sqlx::Error> {
    sqlx::query_as::<_, Speaker>(
        "INSERT INTO speakers (display_name, tenant_id) VALUES ($1, $2) RETURNING *"
    )
    .bind(display_name)
    .bind(tenant_id)
    .fetch_one(pool)
    .await
}

pub async fn get_speaker(pool: &PgPool, id: Uuid) -> Result<Speaker, sqlx::Error> {
    sqlx::query_as::<_, Speaker>("SELECT * FROM speakers WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn list_speakers(pool: &PgPool, tenant_id: Uuid) -> Result<Vec<Speaker>, sqlx::Error> {
    sqlx::query_as::<_, Speaker>(
        "SELECT * FROM speakers WHERE tenant_id = $1 AND status = 'active' ORDER BY created_at DESC"
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}
