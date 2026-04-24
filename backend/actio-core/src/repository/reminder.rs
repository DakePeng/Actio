use std::collections::HashMap;

use sqlx::SqlitePool;
use uuid::Uuid;

use crate::domain::types::{
    NewReminder, PatchReminderRequest, Reminder, ReminderFilter, ReminderRow,
};

// ── helpers ──────────────────────────────────────────────────────────────

/// Compute the new archived_at value given a status transition.
/// Returns Some(true) = set now(), Some(false) = clear, None = no change.
///
/// `'pending'` is a review-queue state: not archived, not active, so
/// transitions in and out of it clear `archived_at` just like `open` does.
/// A user dismissing a pending item goes `pending → archived`, which still
/// sets the timestamp.
pub fn archived_at_for_status(old_status: &str, new_status: &str) -> Option<bool> {
    // Same-status transitions are no-ops first so later arms don't wrongly
    // clear / set on pending→pending or completed→completed.
    if old_status == new_status {
        return None;
    }
    match new_status {
        "archived" => Some(true), // moved into the archive → stamp now()
        "open" | "completed" | "pending" => Some(false), // back to active/review
        _ => None,
    }
}

// ── label join helpers ────────────────────────────────────────────────────

/// Fetch label IDs for a batch of reminder IDs.
/// Returns a map: reminder_id → Vec<label_id>.
pub async fn fetch_label_ids(
    pool: &SqlitePool,
    reminder_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Uuid>>, sqlx::Error> {
    if reminder_ids.is_empty() {
        return Ok(HashMap::new());
    }

    // Build dynamic IN (...) clause
    let placeholders: Vec<String> = (1..=reminder_ids.len())
        .map(|i| format!("?{}", i))
        .collect();
    let sql = format!(
        "SELECT reminder_id, label_id FROM reminder_labels WHERE reminder_id IN ({})",
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<_, (String, String)>(&sql);
    for id in reminder_ids {
        query = query.bind(id.to_string());
    }

    let rows: Vec<(String, String)> = query.fetch_all(pool).await?;

    let mut map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (rid_str, lid_str) in rows {
        if let (Ok(rid), Ok(lid)) = (Uuid::parse_str(&rid_str), Uuid::parse_str(&lid_str)) {
            map.entry(rid).or_default().push(lid);
        }
    }
    Ok(map)
}

/// Replace all labels for a reminder atomically within an existing transaction.
pub async fn replace_reminder_labels(
    txn: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    reminder_id: Uuid,
    label_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM reminder_labels WHERE reminder_id = ?1")
        .bind(reminder_id.to_string())
        .execute(&mut **txn)
        .await?;

    for &lid in label_ids {
        sqlx::query(
            "INSERT INTO reminder_labels (reminder_id, label_id) VALUES (?1, ?2) ON CONFLICT DO NOTHING",
        )
        .bind(reminder_id.to_string())
        .bind(lid.to_string())
        .execute(&mut **txn)
        .await?;
    }
    Ok(())
}

// ── CRUD ─────────────────────────────────────────────────────────────────

pub async fn list_reminders(
    pool: &SqlitePool,
    tenant_id: Uuid,
    filter: &ReminderFilter,
) -> Result<Vec<Reminder>, sqlx::Error> {
    let rows: Vec<ReminderRow> = sqlx::query_as(
        r#"SELECT * FROM reminders
           WHERE tenant_id = ?1
             AND (?2 IS NULL OR status = ?2)
             AND (?3 IS NULL OR priority = ?3)
             AND (?4 IS NULL OR EXISTS (
                     SELECT 1 FROM reminder_labels
                     WHERE reminder_id = reminders.id AND label_id = ?4))
             AND (?5 IS NULL OR
                     title LIKE '%' || ?5 || '%' OR
                     description LIKE '%' || ?5 || '%')
             AND (?6 IS NULL OR session_id = ?6)
           ORDER BY created_at DESC
           LIMIT ?7 OFFSET ?8"#,
    )
    .bind(tenant_id.to_string())
    .bind(&filter.status)
    .bind(&filter.priority)
    .bind(filter.label_id.map(|u| u.to_string()))
    .bind(&filter.search)
    .bind(filter.session_id.map(|u| u.to_string()))
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await?;

    let ids: Vec<Uuid> = rows.iter().filter_map(|r| r.id.parse().ok()).collect();
    let mut labels_map = fetch_label_ids(pool, &ids).await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let key = r.id.parse::<Uuid>().unwrap_or_default();
            let labels = labels_map.remove(&key).unwrap_or_default();
            r.into_reminder(labels)
        })
        .collect())
}

pub async fn create_reminder(
    pool: &SqlitePool,
    item: &NewReminder,
    label_ids: &[Uuid],
) -> Result<Reminder, sqlx::Error> {
    let mut txn = pool.begin().await?;
    let id = Uuid::new_v4().to_string();

    let status = item.status.as_deref().unwrap_or("open");
    let row: ReminderRow = sqlx::query_as(
        r#"INSERT INTO reminders
               (id, session_id, tenant_id, speaker_id, assigned_to, title, description,
                priority, due_time, transcript_excerpt, context, source_time,
                source_window_id, status)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
           RETURNING *"#,
    )
    .bind(&id)
    .bind(item.session_id.map(|u| u.to_string()))
    .bind(item.tenant_id.to_string())
    .bind(item.speaker_id.map(|u| u.to_string()))
    .bind(&item.assigned_to)
    .bind(&item.title)
    .bind(&item.description)
    .bind(&item.priority)
    .bind(item.due_time)
    .bind(&item.transcript_excerpt)
    .bind(&item.context)
    .bind(item.source_time)
    .bind(item.source_window_id.map(|u| u.to_string()))
    .bind(status)
    .fetch_one(&mut *txn)
    .await?;

    let row_uuid = row.id.parse::<Uuid>().unwrap_or_default();
    replace_reminder_labels(&mut txn, row_uuid, label_ids).await?;
    txn.commit().await?;

    Ok(row.into_reminder(label_ids.to_vec()))
}

pub async fn get_reminder(pool: &SqlitePool, id: Uuid) -> Result<Option<Reminder>, sqlx::Error> {
    let row: Option<ReminderRow> = sqlx::query_as("SELECT * FROM reminders WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;

    match row {
        None => Ok(None),
        Some(r) => {
            let r_uuid = r.id.parse::<Uuid>().unwrap_or_default();
            let labels = fetch_label_ids(pool, &[r_uuid]).await?;
            let label_ids = labels.get(&r_uuid).cloned().unwrap_or_default();
            Ok(Some(r.into_reminder(label_ids)))
        }
    }
}

pub async fn patch_reminder(
    pool: &SqlitePool,
    id: Uuid,
    patch: &PatchReminderRequest,
) -> Result<Option<Reminder>, sqlx::Error> {
    // Fetch current status for archived_at transition logic
    let current: Option<(String,)> = sqlx::query_as("SELECT status FROM reminders WHERE id = ?1")
        .bind(id.to_string())
        .fetch_optional(pool)
        .await?;

    let current_status = match current {
        None => return Ok(None),
        Some((s,)) => s,
    };

    let new_status = patch.status.as_deref().unwrap_or(&current_status);
    let set_archived_at = archived_at_for_status(&current_status, new_status);

    let archived_at_sql = match set_archived_at {
        Some(true) => "datetime('now')",
        Some(false) => "NULL",
        None => "archived_at",
    };

    let sql = format!(
        r#"UPDATE reminders SET
               title       = COALESCE(?1, title),
               description = COALESCE(?2, description),
               priority    = COALESCE(?3, priority),
               due_time    = COALESCE(?4, due_time),
               status      = COALESCE(?5, status),
               archived_at = {archived_at_sql},
               updated_at  = datetime('now')
           WHERE id = ?6
           RETURNING *"#,
    );

    let mut txn = pool.begin().await?;

    let row: Option<ReminderRow> = sqlx::query_as(&sql)
        .bind(&patch.title)
        .bind(&patch.description)
        .bind(&patch.priority)
        .bind(patch.due_time)
        .bind(&patch.status)
        .bind(id.to_string())
        .fetch_optional(&mut *txn)
        .await?;

    let row = match row {
        None => {
            txn.rollback().await?;
            return Ok(None);
        }
        Some(r) => r,
    };

    let row_uuid = row.id.parse::<Uuid>().unwrap_or_default();
    if let Some(ref new_labels) = patch.labels {
        replace_reminder_labels(&mut txn, row_uuid, new_labels).await?;
    }

    txn.commit().await?;

    // Re-fetch labels for the response
    let labels = fetch_label_ids(pool, &[row_uuid]).await?;
    let label_ids = labels.get(&row_uuid).cloned().unwrap_or_default();
    Ok(Some(row.into_reminder(label_ids)))
}

pub async fn delete_reminder(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM reminders WHERE id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ── todo_generator compat ─────────────────────────────────────────────────

/// Idempotency check used by todo_generator.
pub async fn has_reminders(pool: &SqlitePool, session_id: Uuid) -> Result<bool, sqlx::Error> {
    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM reminders WHERE session_id = ?1)")
            .bind(session_id.to_string())
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

/// Batch insert reminders from LLM output (no labels, idempotent via ON CONFLICT).
/// Honors `NewReminder.status` (default 'open') and `source_window_id`, so
/// the window extractor can park medium-confidence items with status 'pending'
/// and record their originating window for the trace inspector.
pub async fn create_reminders_batch(
    pool: &SqlitePool,
    items: &[NewReminder],
) -> Result<Vec<Reminder>, sqlx::Error> {
    let mut results = Vec::with_capacity(items.len());
    for item in items {
        let id = Uuid::new_v4().to_string();
        let status = item.status.as_deref().unwrap_or("open");
        let row: Option<ReminderRow> = sqlx::query_as(
            r#"INSERT INTO reminders
                   (id, session_id, tenant_id, speaker_id, assigned_to, title, description,
                    priority, due_time, transcript_excerpt, context, source_time,
                    source_window_id, status)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
               ON CONFLICT (session_id, description) DO NOTHING
               RETURNING *"#,
        )
        .bind(&id)
        .bind(item.session_id.map(|u| u.to_string()))
        .bind(item.tenant_id.to_string())
        .bind(item.speaker_id.map(|u| u.to_string()))
        .bind(&item.assigned_to)
        .bind(&item.title)
        .bind(&item.description)
        .bind(&item.priority)
        .bind(item.due_time)
        .bind(&item.transcript_excerpt)
        .bind(&item.context)
        .bind(item.source_time)
        .bind(item.source_window_id.map(|u| u.to_string()))
        .bind(status)
        .fetch_optional(pool)
        .await?;

        if let Some(r) = row {
            results.push(r.into_reminder(vec![]));
        }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_to_archived_sets_timestamp() {
        assert_eq!(archived_at_for_status("open", "archived"), Some(true));
    }

    #[test]
    fn archived_to_open_clears_timestamp() {
        assert_eq!(archived_at_for_status("archived", "open"), Some(false));
    }

    #[test]
    fn open_to_completed_clears_timestamp() {
        assert_eq!(archived_at_for_status("open", "completed"), Some(false));
    }

    #[test]
    fn archived_to_archived_no_change() {
        assert_eq!(archived_at_for_status("archived", "archived"), None);
    }

    #[test]
    fn pending_transitions_clear_archived_at() {
        // User promoting a review-queue item to active, or a new auto-extract
        // landing in pending, both need archived_at cleared. The catch-all
        // `pending → archived` still sets the timestamp.
        assert_eq!(archived_at_for_status("open", "pending"), Some(false));
        assert_eq!(archived_at_for_status("pending", "open"), Some(false));
        assert_eq!(archived_at_for_status("pending", "archived"), Some(true));
        assert_eq!(archived_at_for_status("pending", "pending"), None);
    }
}
