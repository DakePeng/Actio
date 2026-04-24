//! Extraction-window persistence. One row per (session, start_ms) slice
//! of transcripts that the window extractor scheduled for LLM processing.
//!
//! The table is the source-of-truth for idempotency: the scheduler enumerates
//! candidate windows and inserts new ones with `status='pending'`, then
//! claims the next-pending row by flipping it to `running` in a single
//! atomic UPDATE … RETURNING. Success / empty-input / failure are all
//! terminal, so a crashed worker leaves a `running` row that the caller is
//! responsible for requeuing on startup (`requeue_stale_running`).

use chrono::{DateTime, Utc};
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct ExtractionWindowRow {
    pub id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// Public view used by the extractor and the trace endpoint.
#[derive(Debug, Clone)]
pub struct ExtractionWindow {
    pub id: Uuid,
    pub session_id: Uuid,
    pub start_ms: i64,
    pub end_ms: i64,
    pub status: String,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl ExtractionWindowRow {
    pub fn into_window(self) -> ExtractionWindow {
        ExtractionWindow {
            id: Uuid::parse_str(&self.id).unwrap_or_default(),
            session_id: Uuid::parse_str(&self.session_id).unwrap_or_default(),
            start_ms: self.start_ms,
            end_ms: self.end_ms,
            status: self.status,
            attempts: self.attempts,
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

/// Idempotent insert — returns `true` if a new row was created, `false` if
/// the (session_id, start_ms) pair already existed.
pub async fn upsert_pending_window(
    pool: &SqlitePool,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
) -> Result<bool, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let rows = sqlx::query(
        r#"INSERT INTO extraction_windows (id, session_id, start_ms, end_ms, status)
           VALUES (?1, ?2, ?3, ?4, 'pending')
           ON CONFLICT (session_id, start_ms) DO NOTHING"#,
    )
    .bind(&id)
    .bind(session_id.to_string())
    .bind(start_ms)
    .bind(end_ms)
    .execute(pool)
    .await?;
    Ok(rows.rows_affected() > 0)
}

/// Atomically promote the oldest pending window to `running` and return it.
/// Returns `None` if no pending work exists.
pub async fn claim_next_pending(
    pool: &SqlitePool,
) -> Result<Option<ExtractionWindow>, sqlx::Error> {
    // Use a subquery to pick the oldest pending row, then update it. SQLite
    // doesn't support `UPDATE … ORDER BY … LIMIT RETURNING`, so the shape is
    // `UPDATE … WHERE id = (SELECT id … LIMIT 1) RETURNING *`.
    let row: Option<ExtractionWindowRow> = sqlx::query_as(
        r#"UPDATE extraction_windows
           SET status = 'running', attempts = attempts + 1
           WHERE id = (
               SELECT id FROM extraction_windows
               WHERE status = 'pending'
               ORDER BY created_at, start_ms
               LIMIT 1
           )
           RETURNING *"#,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.into_window()))
}

/// Terminal success — window produced zero-or-more reminders.
pub async fn mark_succeeded(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE extraction_windows
           SET status = 'succeeded',
               finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Terminal empty — transcripts in-window were below the extractable-content
/// threshold so no LLM call was made. Distinct from `succeeded` so ops can
/// tell "quiet window" apart from "LLM looked and found nothing".
pub async fn mark_empty(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE extraction_windows
           SET status = 'empty',
               finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// Record failure. Callers bump `attempts` via `claim_next_pending` on
/// each claim; if `attempts >= MAX_ATTEMPTS`, the caller moves the row to
/// this terminal state. Transient failures (LLM disabled, network) should
/// instead be reverted to `pending` via `revert_to_pending` so a later
/// tick tries again.
pub async fn mark_failed(pool: &SqlitePool, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE extraction_windows
           SET status = 'failed',
               last_error = ?2,
               finished_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
           WHERE id = ?1"#,
    )
    .bind(id.to_string())
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Revert `running` → `pending` so the next tick re-tries. Used when the
/// LLM is temporarily unavailable (disabled, rate-limited) and retrying
/// immediately would just hammer. Does not bump `attempts` since the work
/// never really started.
pub async fn revert_to_pending(
    pool: &SqlitePool,
    id: Uuid,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"UPDATE extraction_windows
           SET status = 'pending',
               last_error = ?2,
               attempts = attempts - 1
           WHERE id = ?1 AND status = 'running'"#,
    )
    .bind(id.to_string())
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

/// Startup housekeeping: windows left in `running` when the process last
/// exited will never self-resolve. Revert them to `pending` so the next
/// scheduler tick picks them back up. Returns the number of rows touched.
pub async fn requeue_stale_running(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"UPDATE extraction_windows
           SET status = 'pending',
               last_error = 'orphaned by restart'
           WHERE status = 'running'"#,
    )
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn get_window(
    pool: &SqlitePool,
    id: Uuid,
) -> Result<Option<ExtractionWindow>, sqlx::Error> {
    let row: Option<ExtractionWindowRow> =
        sqlx::query_as("SELECT * FROM extraction_windows WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.into_window()))
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
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

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
    async fn upsert_then_claim_flips_to_running() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;

        let first = upsert_pending_window(&pool, sid, 0, 300_000).await.unwrap();
        assert!(first, "first insert should be a new row");

        let dup = upsert_pending_window(&pool, sid, 0, 300_000).await.unwrap();
        assert!(!dup, "duplicate (session, start_ms) must not insert");

        let claimed = claim_next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(claimed.session_id, sid);
        assert_eq!(claimed.start_ms, 0);
        assert_eq!(claimed.status, "running");
        assert_eq!(claimed.attempts, 1);

        // No more pending work.
        assert!(claim_next_pending(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn terminal_transitions_set_finished_at() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        upsert_pending_window(&pool, sid, 0, 300_000).await.unwrap();
        let w = claim_next_pending(&pool).await.unwrap().unwrap();

        mark_succeeded(&pool, w.id).await.unwrap();
        let got = get_window(&pool, w.id).await.unwrap().unwrap();
        assert_eq!(got.status, "succeeded");
        assert!(got.finished_at.is_some());
    }

    #[tokio::test]
    async fn revert_to_pending_decrements_attempts() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        upsert_pending_window(&pool, sid, 0, 300_000).await.unwrap();
        let w = claim_next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(w.attempts, 1);

        revert_to_pending(&pool, w.id, "llm disabled")
            .await
            .unwrap();
        let got = get_window(&pool, w.id).await.unwrap().unwrap();
        assert_eq!(got.status, "pending");
        assert_eq!(
            got.attempts, 0,
            "revert must not count against retry budget"
        );
    }

    #[tokio::test]
    async fn requeue_stale_running_resurrects_orphans() {
        let pool = fresh_pool().await;
        let sid = mk_session(&pool).await;
        upsert_pending_window(&pool, sid, 0, 300_000).await.unwrap();
        claim_next_pending(&pool).await.unwrap().unwrap();
        // Pretend the process died mid-flight — row is stuck in 'running'.

        let moved = requeue_stale_running(&pool).await.unwrap();
        assert_eq!(moved, 1);

        let w = claim_next_pending(&pool).await.unwrap().unwrap();
        assert_eq!(w.status, "running");
    }
}
