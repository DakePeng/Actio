use crate::domain::types::Speaker;
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn create_speaker(
    pool: &SqlitePool,
    display_name: &str,
    color: &str,
    tenant_id: Uuid,
) -> Result<Speaker, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query_as::<_, Speaker>(
        "INSERT INTO speakers (id, display_name, color, tenant_id) \
         VALUES (?1, ?2, ?3, ?4) RETURNING *",
    )
    .bind(&id)
    .bind(display_name)
    .bind(color)
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
        "SELECT * FROM speakers WHERE tenant_id = ?1 AND status = 'active' \
         ORDER BY created_at DESC",
    )
    .bind(tenant_id.to_string())
    .fetch_all(pool)
    .await
}

pub async fn update_speaker(
    pool: &SqlitePool,
    id: Uuid,
    display_name: Option<&str>,
    color: Option<&str>,
) -> Result<Option<Speaker>, sqlx::Error> {
    // COALESCE lets us pass NULL for "keep existing" per field.
    sqlx::query_as::<_, Speaker>(
        "UPDATE speakers \
         SET display_name = COALESCE(?1, display_name), \
             color = COALESCE(?2, color) \
         WHERE id = ?3 RETURNING *",
    )
    .bind(display_name)
    .bind(color)
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
}

/// Hard delete: removes the speaker and cascades to speaker_embeddings via FK.
/// Callers MUST first null-out audio_segments.speaker_id to avoid dangling FKs
/// (SQLite's REFERENCES clause on audio_segments does not have ON DELETE SET NULL
/// in the existing schema). See `delete_speaker_with_segment_cleanup` below.
pub async fn hard_delete_speaker(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Single-transaction delete with segment FK cleanup.
pub async fn delete_speaker_with_segment_cleanup(
    pool: &SqlitePool,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE audio_segments SET speaker_id = NULL WHERE speaker_id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("DELETE FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn create_speaker_persists_color() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        assert_eq!(s.display_name, "Alice");
        assert_eq!(s.color, "#E57373");
    }

    #[tokio::test]
    async fn update_speaker_applies_patch() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let updated = update_speaker(
            &pool,
            Uuid::parse_str(&s.id).unwrap(),
            Some("Alicia"),
            Some("#64B5F6"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(updated.display_name, "Alicia");
        assert_eq!(updated.color, "#64B5F6");
    }
}
