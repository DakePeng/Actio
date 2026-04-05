use crate::domain::types::AudioSession;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_session(
    pool: &PgPool,
    tenant_id: Uuid,
    source_type: &str,
    mode: &str,
) -> Result<AudioSession, sqlx::Error> {
    sqlx::query_as::<_, AudioSession>(
        "INSERT INTO audio_sessions (tenant_id, source_type, mode) VALUES ($1, $2, $3) RETURNING *"
    )
    .bind(tenant_id)
    .bind(source_type)
    .bind(mode)
    .fetch_one(pool)
    .await
}

pub async fn get_session(pool: &PgPool, id: Uuid) -> Result<AudioSession, sqlx::Error> {
    sqlx::query_as::<_, AudioSession>("SELECT * FROM audio_sessions WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
}

pub async fn end_session(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE audio_sessions SET ended_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
