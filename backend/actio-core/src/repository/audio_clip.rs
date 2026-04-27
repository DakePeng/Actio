//! Audio clip persistence — one row per ~5-min recorded clip on disk.
//! Status cycles `pending → running → processed | empty | failed`. Crash
//! recovery: `requeue_stale_running` reverts orphans on startup. Mirrors
//! the shape of `repository::extraction_window` for the legacy time-window
//! flow.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use crate::domain::types::AudioClip;

#[derive(Debug, Clone, FromRow)]
pub struct AudioClipRow {
    pub id: String,
    pub session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub segment_count: i64,
    pub manifest_path: String,
    pub status: String,
    pub attempts: i64,
    pub archive_model: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

impl AudioClipRow {
    pub fn into_clip(self) -> AudioClip {
        AudioClip {
            id: Uuid::parse_str(&self.id).unwrap_or_default(),
            session_id: Uuid::parse_str(&self.session_id).unwrap_or_default(),
            started_at_ms: self.started_at_ms,
            ended_at_ms: self.ended_at_ms,
            segment_count: self.segment_count,
            manifest_path: self.manifest_path,
            status: self.status,
            attempts: self.attempts,
            archive_model: self.archive_model,
            last_error: self.last_error,
            created_at: DateTime::parse_from_rfc3339(&self.created_at)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            finished_at: self.finished_at.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|d| d.with_timezone(&Utc))
            }),
        }
    }
}

/// Insert a fresh clip row in `pending`. Returns the new id.
pub async fn insert_pending(
    pool: &SqlitePool,
    session_id: Uuid,
    started_at_ms: i64,
    ended_at_ms: i64,
    segment_count: i64,
    manifest_path: &str,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"INSERT INTO audio_clips
           (id, session_id, started_at_ms, ended_at_ms, segment_count,
            manifest_path, status, attempts)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0)"#,
    )
    .bind(id.to_string())
    .bind(session_id.to_string())
    .bind(started_at_ms)
    .bind(ended_at_ms)
    .bind(segment_count)
    .bind(manifest_path)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Atomically promote the oldest pending clip to `running` and return it.
/// Skips clips that have already failed `attempts >= 3` times.
pub async fn claim_next_pending(pool: &SqlitePool) -> Result<Option<AudioClip>, sqlx::Error> {
    let row: Option<AudioClipRow> = sqlx::query_as(
        r#"UPDATE audio_clips
           SET status = 'running', attempts = attempts + 1
           WHERE id = (
               SELECT id FROM audio_clips
               WHERE status = 'pending' AND attempts < 3
               ORDER BY started_at_ms ASC
               LIMIT 1
           )
           RETURNING *"#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(AudioClipRow::into_clip))
}

/// Boot housekeeping — any `running` row from a prior process crash should
/// be reverted to `pending` so the BatchProcessor picks it up again.
pub async fn requeue_stale_running(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"UPDATE audio_clips SET status = 'pending'
           WHERE status = 'running'"#,
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Terminal success — clip transcribed, clustered, action items queued.
pub async fn mark_processed(
    pool: &SqlitePool,
    id: Uuid,
    archive_model: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE audio_clips
           SET status = 'processed',
               archive_model = COALESCE(?2, archive_model),
               finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .bind(archive_model)
    .execute(pool)
    .await?;
    Ok(())
}

/// Terminal empty — manifest had zero segments, no LLM call needed.
pub async fn mark_empty(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE audio_clips
           SET status = 'empty',
               finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Record failure. After 3 attempts the row is parked in terminal `failed`;
/// before that it reverts to `pending` so the BatchProcessor re-tries.
pub async fn mark_failed(pool: &SqlitePool, id: Uuid, err: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE audio_clips
           SET status = CASE WHEN attempts >= 3 THEN 'failed' ELSE 'pending' END,
               last_error = ?2,
               finished_at = CASE WHEN attempts >= 3
                   THEN strftime('%Y-%m-%dT%H:%M:%fZ','now') ELSE finished_at END
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .bind(err)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_by_id(pool: &SqlitePool, id: Uuid) -> Result<Option<AudioClip>, sqlx::Error> {
    let row: Option<AudioClipRow> = sqlx::query_as(r#"SELECT * FROM audio_clips WHERE id = ?1"#)
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row.map(AudioClipRow::into_clip))
}

/// One processed clip with its joined transcript text, for the Archive view.
/// `text` is the concatenation of every final transcript across all segments
/// in the clip, space-separated. Empty string for clips with zero finals.
#[derive(Debug, Clone, FromRow)]
pub struct ClipArchiveRow {
    pub id: String,
    pub session_id: String,
    pub created_at: String,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub text: Option<String>,
}

/// List the most recent processed clips with their joined transcript text.
/// Used by `GET /clips` to populate the Archive view's Clips section.
///
/// Filters:
///   * status = 'processed' or 'empty' — clips that finished the batch pass
///     (failed clips aren't shown; pending/running aren't yet useful)
///   * ordered by started_at_ms DESC so newest is first
///   * limit caps the row count to avoid sending megabytes of joined text
pub async fn list_recent_with_text(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<ClipArchiveRow>, sqlx::Error> {
    sqlx::query_as::<_, ClipArchiveRow>(
        r#"
        SELECT
            c.id,
            c.session_id,
            c.created_at,
            c.started_at_ms,
            c.ended_at_ms,
            (
                SELECT GROUP_CONCAT(t.text, ' ')
                FROM transcripts t
                JOIN audio_segments s ON s.id = t.segment_id
                WHERE s.clip_id = c.id AND t.is_final = 1
            ) AS text
        FROM audio_clips c
        WHERE c.status IN ('processed', 'empty')
        ORDER BY c.started_at_ms DESC
        LIMIT ?1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::fresh_pool;

    async fn mk_session(pool: &SqlitePool) -> Uuid {
        let sid = Uuid::new_v4();
        sqlx::query(
            r#"INSERT INTO audio_sessions (id, tenant_id, source_type, mode, routing_policy)
               VALUES (?1, '00000000-0000-0000-0000-000000000000', 'microphone', 'realtime', 'default')"#,
        )
        .bind(sid.to_string())
        .execute(pool)
        .await
        .unwrap();
        sid
    }

    #[tokio::test]
    async fn insert_pending_then_claim_marks_running() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        let id = insert_pending(
            &pool,
            session_id,
            1_000,
            301_000,
            5,
            "/tmp/foo/manifest.json",
        )
        .await
        .unwrap();

        let claimed = claim_next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(claimed.id, id);
        assert_eq!(claimed.status, "running");
        assert_eq!(claimed.attempts, 1);

        // Idempotency — no second claim.
        assert!(claim_next_pending(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn requeue_stale_running_reverts_orphans() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let id = insert_pending(&pool, session_id, 0, 300_000, 3, "/tmp/m.json")
            .await
            .unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();

        let n = requeue_stale_running(&pool).await.unwrap();
        assert_eq!(n, 1);
        let clip = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(clip.status, "pending");
    }

    #[tokio::test]
    async fn mark_processed_sets_finished_at_and_records_model() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let id = insert_pending(&pool, session_id, 0, 300_000, 3, "/tmp/m.json")
            .await
            .unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();
        mark_processed(&pool, id, Some("whisper-medium"))
            .await
            .unwrap();
        let clip = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(clip.status, "processed");
        assert!(clip.finished_at.is_some());
        assert_eq!(clip.archive_model.as_deref(), Some("whisper-medium"));
    }

    #[tokio::test]
    async fn mark_failed_third_attempt_terminalizes() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let id = insert_pending(&pool, session_id, 0, 300_000, 1, "/tmp/m.json")
            .await
            .unwrap();
        // attempt 1 → fail → reverted to pending
        let _ = claim_next_pending(&pool).await.unwrap();
        mark_failed(&pool, id, "boom").await.unwrap();
        assert_eq!(
            get_by_id(&pool, id).await.unwrap().unwrap().status,
            "pending"
        );
        // attempt 2 → fail → still pending
        let _ = claim_next_pending(&pool).await.unwrap();
        mark_failed(&pool, id, "boom2").await.unwrap();
        assert_eq!(
            get_by_id(&pool, id).await.unwrap().unwrap().status,
            "pending"
        );
        // attempt 3 → fail → terminal failed
        let _ = claim_next_pending(&pool).await.unwrap();
        mark_failed(&pool, id, "boom3").await.unwrap();
        let clip = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(clip.status, "failed");
        assert_eq!(clip.last_error.as_deref(), Some("boom3"));
        assert!(clip.finished_at.is_some());
    }

    #[tokio::test]
    async fn list_recent_with_text_returns_processed_and_empty_only() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;

        // 3 clips: pending, processed, empty. Only the latter two should
        // appear in the archive list.
        let pending_id = insert_pending(&pool, session_id, 0, 1_000, 0, "/tmp/p.json")
            .await
            .unwrap();
        let processed_id = insert_pending(&pool, session_id, 1_000, 2_000, 1, "/tmp/q.json")
            .await
            .unwrap();
        let empty_id = insert_pending(&pool, session_id, 2_000, 3_000, 0, "/tmp/r.json")
            .await
            .unwrap();

        // Promote and terminalize the latter two.
        let _ = claim_next_pending(&pool).await.unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();
        // claim_next_pending picks ORDER BY started_at_ms — first claim is
        // pending_id (0), second is processed_id (1_000), third is empty_id.
        // We intentionally leave pending_id in 'running' so it doesn't match.
        mark_processed(&pool, processed_id, None).await.unwrap();
        mark_empty(&pool, empty_id).await.unwrap();
        // pending_id was claimed but we didn't mark it; it stays running.
        // The list query shouldn't return it.

        let rows = list_recent_with_text(&pool, 50).await.unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&processed_id.to_string().as_str()));
        assert!(ids.contains(&empty_id.to_string().as_str()));
        assert!(!ids.contains(&pending_id.to_string().as_str()));
        // Newest first: empty_id started later than processed_id.
        assert_eq!(rows[0].id, empty_id.to_string());
    }

    #[tokio::test]
    async fn list_recent_with_text_concatenates_transcripts() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let clip_id = insert_pending(&pool, session_id, 0, 5_000, 2, "/tmp/m.json")
            .await
            .unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();
        mark_processed(&pool, clip_id, None).await.unwrap();

        // Insert two segments tied to the clip + a final transcript on each.
        let seg1 = Uuid::new_v4().to_string();
        let seg2 = Uuid::new_v4().to_string();
        sqlx::query(
            r#"INSERT INTO audio_segments (id, session_id, start_ms, end_ms, clip_id)
               VALUES (?1, ?2, 0, 1_000, ?3), (?4, ?2, 1_000, 2_000, ?3)"#,
        )
        .bind(&seg1)
        .bind(session_id.to_string())
        .bind(clip_id.to_string())
        .bind(&seg2)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO transcripts
               (id, session_id, segment_id, start_ms, end_ms, text, is_final, backend_type)
               VALUES
               (?1, ?2, ?3, 0, 1_000, 'hello world', 1, 'local'),
               (?4, ?2, ?5, 1_000, 2_000, 'goodbye', 1, 'local')"#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(session_id.to_string())
        .bind(&seg1)
        .bind(Uuid::new_v4().to_string())
        .bind(&seg2)
        .execute(&pool)
        .await
        .unwrap();

        let rows = list_recent_with_text(&pool, 50).await.unwrap();
        let row = rows.iter().find(|r| r.id == clip_id.to_string()).unwrap();
        // GROUP_CONCAT order is unspecified by SQL but SQLite typically
        // preserves table-row order; assert both substrings present.
        let text = row.text.clone().unwrap_or_default();
        assert!(text.contains("hello world"), "got {text:?}");
        assert!(text.contains("goodbye"), "got {text:?}");
    }

    #[tokio::test]
    async fn mark_empty_terminalizes_zero_segment_clips() {
        let pool = fresh_pool().await;
        let session_id = mk_session(&pool).await;
        let id = insert_pending(&pool, session_id, 0, 300_000, 0, "/tmp/m.json")
            .await
            .unwrap();
        let _ = claim_next_pending(&pool).await.unwrap();
        mark_empty(&pool, id).await.unwrap();
        let clip = get_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(clip.status, "empty");
        assert!(clip.finished_at.is_some());
    }
}
