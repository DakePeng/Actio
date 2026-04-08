use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::types::{NewReminder, PatchReminderRequest, Reminder, ReminderFilter, ReminderRow};

// ── helpers ──────────────────────────────────────────────────────────────

/// Compute the new archived_at value given a status transition.
/// Returns Some(true) = set now(), Some(false) = clear, None = no change.
pub fn archived_at_for_status(old_status: &str, new_status: &str) -> Option<bool> {
    match (old_status, new_status) {
        (s, "archived") if s != "archived" => Some(true),  // set now()
        (_, "open") | (_, "completed") => Some(false),     // clear
        _ => None,                                          // no change
    }
}

// ── label join helpers ────────────────────────────────────────────────────

/// Fetch label IDs for a batch of reminder IDs.
/// Returns a map: reminder_id → Vec<label_id>.
pub async fn fetch_label_ids(
    pool: &PgPool,
    reminder_ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<Uuid>>, sqlx::Error> {
    if reminder_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT reminder_id, label_id FROM reminder_labels WHERE reminder_id = ANY($1)",
    )
    .bind(reminder_ids)
    .fetch_all(pool)
    .await?;

    let mut map: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (rid, lid) in rows {
        map.entry(rid).or_default().push(lid);
    }
    Ok(map)
}

/// Replace all labels for a reminder atomically within an existing transaction.
pub async fn replace_reminder_labels(
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    reminder_id: Uuid,
    label_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM reminder_labels WHERE reminder_id = $1")
        .bind(reminder_id)
        .execute(&mut **txn)
        .await?;

    for &lid in label_ids {
        sqlx::query(
            "INSERT INTO reminder_labels (reminder_id, label_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(reminder_id)
        .bind(lid)
        .execute(&mut **txn)
        .await?;
    }
    Ok(())
}

// ── CRUD ─────────────────────────────────────────────────────────────────

pub async fn list_reminders(
    pool: &PgPool,
    tenant_id: Uuid,
    filter: &ReminderFilter,
) -> Result<Vec<Reminder>, sqlx::Error> {
    let rows: Vec<ReminderRow> = sqlx::query_as(
        r#"SELECT * FROM reminders
           WHERE tenant_id = $1
             AND ($2::varchar IS NULL OR status = $2)
             AND ($3::varchar IS NULL OR priority = $3)
             AND ($4::uuid IS NULL OR EXISTS (
                     SELECT 1 FROM reminder_labels
                     WHERE reminder_id = reminders.id AND label_id = $4))
             AND ($5::text IS NULL OR
                     title ILIKE '%' || $5 || '%' OR
                     description ILIKE '%' || $5 || '%')
             AND ($6::uuid IS NULL OR session_id = $6)
           ORDER BY created_at DESC
           LIMIT $7 OFFSET $8"#,
    )
    .bind(tenant_id)
    .bind(&filter.status)
    .bind(&filter.priority)
    .bind(filter.label_id)
    .bind(&filter.search)
    .bind(filter.session_id)
    .bind(filter.limit)
    .bind(filter.offset)
    .fetch_all(pool)
    .await?;

    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut labels_map = fetch_label_ids(pool, &ids).await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let labels = labels_map.remove(&r.id).unwrap_or_default();
            r.into_reminder(labels)
        })
        .collect())
}

pub async fn create_reminder(
    pool: &PgPool,
    item: &NewReminder,
    label_ids: &[Uuid],
) -> Result<Reminder, sqlx::Error> {
    let mut txn = pool.begin().await?;

    let row: ReminderRow = sqlx::query_as(
        r#"INSERT INTO reminders
               (session_id, tenant_id, speaker_id, assigned_to, title, description,
                priority, transcript_excerpt, context, source_time)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
           RETURNING *"#,
    )
    .bind(item.session_id)
    .bind(item.tenant_id)
    .bind(item.speaker_id)
    .bind(&item.assigned_to)
    .bind(&item.title)
    .bind(&item.description)
    .bind(&item.priority)
    .bind(&item.transcript_excerpt)
    .bind(&item.context)
    .bind(item.source_time)
    .fetch_one(&mut *txn)
    .await?;

    replace_reminder_labels(&mut txn, row.id, label_ids).await?;
    txn.commit().await?;

    Ok(row.into_reminder(label_ids.to_vec()))
}

pub async fn get_reminder(pool: &PgPool, id: Uuid) -> Result<Option<Reminder>, sqlx::Error> {
    let row: Option<ReminderRow> =
        sqlx::query_as("SELECT * FROM reminders WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    match row {
        None => Ok(None),
        Some(r) => {
            let labels = fetch_label_ids(pool, &[r.id]).await?;
            let label_ids = labels.get(&r.id).cloned().unwrap_or_default();
            Ok(Some(r.into_reminder(label_ids)))
        }
    }
}

pub async fn patch_reminder(
    pool: &PgPool,
    id: Uuid,
    patch: &PatchReminderRequest,
) -> Result<Option<Reminder>, sqlx::Error> {
    // Fetch current status for archived_at transition logic
    let current: Option<(String,)> =
        sqlx::query_as("SELECT status FROM reminders WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    let current_status = match current {
        None => return Ok(None),
        Some((s,)) => s,
    };

    let new_status = patch.status.as_deref().unwrap_or(&current_status);
    let set_archived_at = archived_at_for_status(&current_status, new_status);

    let archived_at_sql = match set_archived_at {
        Some(true) => "now()",
        Some(false) => "NULL",
        None => "archived_at",
    };

    let sql = format!(
        r#"UPDATE reminders SET
               title       = COALESCE($1, title),
               description = COALESCE($2, description),
               priority    = COALESCE($3, priority),
               due_time    = COALESCE($4, due_time),
               status      = COALESCE($5, status),
               archived_at = {archived_at_sql},
               updated_at  = now()
           WHERE id = $6
           RETURNING *"#,
    );

    let mut txn = pool.begin().await?;

    let row: Option<ReminderRow> = sqlx::query_as(&sql)
        .bind(&patch.title)
        .bind(&patch.description)
        .bind(&patch.priority)
        .bind(patch.due_time)
        .bind(&patch.status)
        .bind(id)
        .fetch_optional(&mut *txn)
        .await?;

    let row = match row {
        None => {
            txn.rollback().await?;
            return Ok(None);
        }
        Some(r) => r,
    };

    if let Some(ref new_labels) = patch.labels {
        replace_reminder_labels(&mut txn, row.id, new_labels).await?;
    }

    txn.commit().await?;

    // Re-fetch labels for the response
    let labels = fetch_label_ids(pool, &[row.id]).await?;
    let label_ids = labels.get(&row.id).cloned().unwrap_or_default();
    Ok(Some(row.into_reminder(label_ids)))
}

pub async fn delete_reminder(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM reminders WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// ── todo_generator compat ─────────────────────────────────────────────────

/// Idempotency check used by todo_generator.
pub async fn has_reminders(pool: &PgPool, session_id: Uuid) -> Result<bool, sqlx::Error> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM reminders WHERE session_id = $1)",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Batch insert reminders from LLM output (no labels, idempotent via ON CONFLICT).
pub async fn create_reminders_batch(
    pool: &PgPool,
    items: &[NewReminder],
) -> Result<Vec<Reminder>, sqlx::Error> {
    let mut results = Vec::with_capacity(items.len());
    for item in items {
        let row: Option<ReminderRow> = sqlx::query_as(
            r#"INSERT INTO reminders
                   (session_id, tenant_id, speaker_id, assigned_to, title, description,
                    priority, transcript_excerpt, context, source_time)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               ON CONFLICT (session_id, description) DO NOTHING
               RETURNING *"#,
        )
        .bind(item.session_id)
        .bind(item.tenant_id)
        .bind(item.speaker_id)
        .bind(&item.assigned_to)
        .bind(&item.title)
        .bind(&item.description)
        .bind(&item.priority)
        .bind(&item.transcript_excerpt)
        .bind(&item.context)
        .bind(item.source_time)
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
}
