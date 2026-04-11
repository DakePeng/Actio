use crate::domain::types::AudioSession;
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn create_session(
    pool: &SqlitePool,
    tenant_id: Uuid,
    source_type: &str,
    mode: &str,
) -> Result<AudioSession, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let tenant_id_str = tenant_id.to_string();
    sqlx::query_as::<_, AudioSession>(
        "INSERT INTO audio_sessions (id, tenant_id, source_type, mode) VALUES (?1, ?2, ?3, ?4) RETURNING *"
    )
    .bind(&id)
    .bind(&tenant_id_str)
    .bind(source_type)
    .bind(mode)
    .fetch_one(pool)
    .await
}

pub async fn get_session(pool: &SqlitePool, id: Uuid) -> Result<AudioSession, sqlx::Error> {
    sqlx::query_as::<_, AudioSession>("SELECT * FROM audio_sessions WHERE id = ?1")
        .bind(id.to_string())
        .fetch_one(pool)
        .await
}

pub async fn end_session(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE audio_sessions SET ended_at = datetime('now') WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_sessions(
    pool: &SqlitePool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<AudioSession>, sqlx::Error> {
    sqlx::query_as::<_, AudioSession>(
        "SELECT * FROM audio_sessions WHERE tenant_id = ?1 ORDER BY started_at DESC LIMIT ?2 OFFSET ?3",
    )
    .bind(tenant_id.to_string())
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}
