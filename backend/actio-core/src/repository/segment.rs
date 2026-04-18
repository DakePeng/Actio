use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UnknownSegmentRow {
    pub id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub embedding: Option<Vec<u8>>,
    pub embedding_dim: Option<i64>,
}

pub async fn list_unknown_segments(
    pool: &SqlitePool,
    session_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<UnknownSegmentRow>, sqlx::Error> {
    if let Some(sid) = session_id {
        sqlx::query_as::<_, UnknownSegmentRow>(
            "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
             FROM audio_segments \
             WHERE session_id = ?1 AND speaker_id IS NULL \
             ORDER BY start_ms LIMIT ?2",
        )
        .bind(sid.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, UnknownSegmentRow>(
            "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
             FROM audio_segments WHERE speaker_id IS NULL \
             ORDER BY start_ms DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

/// Assign a speaker to a segment. Returns the segment row as it was BEFORE
/// the update so the caller can promote the embedding to the speaker's
/// voiceprint collection.
pub async fn assign_speaker(
    pool: &SqlitePool,
    segment_id: Uuid,
    speaker_id: Uuid,
) -> Result<Option<UnknownSegmentRow>, sqlx::Error> {
    let prev = sqlx::query_as::<_, UnknownSegmentRow>(
        "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
         FROM audio_segments WHERE id = ?1",
    )
    .bind(segment_id.to_string())
    .fetch_optional(pool)
    .await?;

    let updated = sqlx::query("UPDATE audio_segments SET speaker_id = ?1 WHERE id = ?2")
        .bind(speaker_id.to_string())
        .bind(segment_id.to_string())
        .execute(pool)
        .await?;
    if updated.rows_affected() == 0 {
        return Ok(None);
    }
    Ok(prev)
}

pub async fn unassign_speaker(pool: &SqlitePool, segment_id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("UPDATE audio_segments SET speaker_id = NULL WHERE id = ?1")
        .bind(segment_id.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Insert an audio_segments row, optionally attaching a pre-computed embedding
/// and speaker identification result. Used by the live inference pipeline as
/// each VAD-detected segment completes.
pub async fn insert_segment(
    pool: &SqlitePool,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    speaker_id: Option<Uuid>,
    speaker_score: Option<f64>,
    embedding: Option<&[f32]>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let (blob, dim) = match embedding {
        Some(e) => (
            Some(bytemuck::cast_slice::<f32, u8>(e).to_vec()),
            Some(e.len() as i64),
        ),
        None => (None, None),
    };
    sqlx::query(
        "INSERT INTO audio_segments \
           (id, session_id, start_ms, end_ms, speaker_id, speaker_score, embedding, embedding_dim) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(&id)
    .bind(session_id.to_string())
    .bind(start_ms)
    .bind(end_ms)
    .bind(speaker_id.map(|u| u.to_string()))
    .bind(speaker_score)
    .bind(blob)
    .bind(dim)
    .execute(pool)
    .await?;
    Uuid::parse_str(&id).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

pub async fn has_primary_embedding(
    pool: &SqlitePool,
    speaker_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?1 AND is_primary = 1",
    )
    .bind(speaker_id.to_string())
    .fetch_one(pool)
    .await?;
    Ok(row.0 > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use crate::repository::speaker::create_speaker;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    async fn insert_session(pool: &SqlitePool) -> String {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO audio_sessions (id) VALUES (?1)")
            .bind(&id)
            .execute(pool)
            .await
            .unwrap();
        id
    }

    async fn insert_unknown_segment(pool: &SqlitePool, session_id: &str, start: i64) -> String {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audio_segments \
               (id, session_id, start_ms, end_ms, embedding, embedding_dim) \
             VALUES (?1, ?2, ?3, ?4, ?5, 512)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(start)
        .bind(start + 1000)
        .bind(bytemuck::cast_slice::<f32, u8>(&vec![0.5f32; 512]))
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn lists_unknowns_per_session() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        insert_unknown_segment(&pool, &sid, 0).await;
        insert_unknown_segment(&pool, &sid, 1000).await;
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows[0].embedding.is_some());
        assert_eq!(rows[0].embedding_dim, Some(512));
    }

    #[tokio::test]
    async fn assigning_returns_previous_row() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        let seg_id = insert_unknown_segment(&pool, &sid, 0).await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();

        let prev = assign_speaker(
            &pool,
            Uuid::parse_str(&seg_id).unwrap(),
            Uuid::parse_str(&alice.id).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(prev.embedding.is_some());
        assert_eq!(prev.embedding_dim, Some(512));

        // Unknown list for the session is now empty.
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10)
            .await
            .unwrap();
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn unassign_nulls_speaker_id() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        let seg_id = insert_unknown_segment(&pool, &sid, 0).await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        assign_speaker(
            &pool,
            Uuid::parse_str(&seg_id).unwrap(),
            Uuid::parse_str(&alice.id).unwrap(),
        )
        .await
        .unwrap();

        assert!(unassign_speaker(&pool, Uuid::parse_str(&seg_id).unwrap())
            .await
            .unwrap());

        // Segment is back in the unknown list.
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
    }
}
