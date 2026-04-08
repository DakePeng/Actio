# Reminders API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the backend with a full Reminders API — migrating `todos` → `reminders`, adding labels, and exposing CRUD endpoints that match the frontend's data model.

**Architecture:** Three new migrations extend the schema. `src/repository/reminder.rs` and `src/repository/label.rs` are new; existing repositories get small additions. New `src/api/reminder.rs` and `src/api/label.rs` hold handlers. `todo_generator.rs` is updated to write through `reminder_repo`. All changes are additive — `todo.rs` is kept for the backward-compat alias route.

**Tech Stack:** Rust (axum 0.7, sqlx 0.8, uuid 1, chrono 0.4, serde, utoipa), PostgreSQL + pgvector

**Spec:** `docs/superpowers/specs/2026-04-08-reminders-api-design.md`

---

## Task 1: Migrations

**Files:**
- Create: `migrations/007_rename_todos_to_reminders.sql`
- Create: `migrations/008_create_labels.sql`
- Create: `migrations/009_create_sessions_index.sql`

- [ ] **Step 1: Create migration 007**

Create `migrations/007_rename_todos_to_reminders.sql`:

```sql
ALTER TABLE todos RENAME TO reminders;

ALTER TABLE reminders
    ADD COLUMN tenant_id          UUID        NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    ADD COLUMN title              TEXT,
    ADD COLUMN due_time           TIMESTAMPTZ,
    ADD COLUMN archived_at        TIMESTAMPTZ,
    ADD COLUMN transcript_excerpt TEXT,
    ADD COLUMN context            TEXT,
    ADD COLUMN source_time        TIMESTAMPTZ;

CREATE INDEX idx_reminders_tenant ON reminders(tenant_id);
CREATE INDEX idx_reminders_status  ON reminders(tenant_id, status);
```

- [ ] **Step 2: Create migration 008**

Create `migrations/008_create_labels.sql`:

```sql
CREATE TABLE labels (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id  UUID        NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    name       TEXT        NOT NULL,
    color      VARCHAR(7)  NOT NULL,
    bg_color   VARCHAR(7)  NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, name)
);

CREATE TABLE reminder_labels (
    reminder_id UUID NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
    label_id    UUID NOT NULL REFERENCES labels(id)    ON DELETE CASCADE,
    PRIMARY KEY (reminder_id, label_id)
);

CREATE INDEX idx_labels_tenant        ON labels(tenant_id);
CREATE INDEX idx_reminder_labels_label ON reminder_labels(label_id);
```

- [ ] **Step 3: Create migration 009**

Create `migrations/009_create_sessions_index.sql`:

```sql
CREATE INDEX idx_sessions_tenant_started
    ON audio_sessions(tenant_id, started_at DESC);
```

- [ ] **Step 4: Verify migrations apply cleanly**

```bash
cd backend
cargo check
```

Expected: compiles. (Migrations run at startup — verify SQL syntax only for now.)

- [ ] **Step 5: Commit**

```bash
git add migrations/
git commit -m "migration: rename todos to reminders, add labels, session index"
```

---

## Task 2: Domain Types

**Files:**
- Modify: `src/domain/types.rs`

Add `ReminderRow`, `Reminder`, `NewReminder`, `ReminderFilter`, `PatchReminderRequest`, `Label`, `CreateLabelRequest`, `PatchLabelRequest`, `ListSessionsParams`.

- [ ] **Step 1: Add types to types.rs**

Append to the END of `src/domain/types.rs`:

```rust
// ── Reminder ─────────────────────────────────────────────────────────────

/// Raw DB row for the reminders table (no joined labels).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReminderRow {
    pub id: Uuid,
    pub session_id: Option<Uuid>,
    pub tenant_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub title: Option<String>,
    pub description: String,
    pub status: String,
    pub priority: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ReminderRow {
    pub fn into_reminder(self, labels: Vec<Uuid>) -> Reminder {
        Reminder {
            id: self.id,
            session_id: self.session_id,
            tenant_id: self.tenant_id,
            speaker_id: self.speaker_id,
            assigned_to: self.assigned_to,
            title: self.title,
            description: self.description,
            status: self.status,
            priority: self.priority,
            due_time: self.due_time,
            archived_at: self.archived_at,
            transcript_excerpt: self.transcript_excerpt,
            context: self.context,
            source_time: self.source_time,
            labels,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// API response type — includes joined label IDs.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Reminder {
    pub id: Uuid,
    pub session_id: Option<Uuid>,
    pub tenant_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub title: Option<String>,
    pub description: String,
    pub status: String,
    pub priority: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    pub labels: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new reminder (API or LLM generator).
#[derive(Debug)]
pub struct NewReminder {
    pub session_id: Option<Uuid>,
    pub tenant_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub title: Option<String>,
    pub description: String,
    pub priority: Option<String>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
}

/// Query parameters for GET /reminders.
#[derive(Debug, Default)]
pub struct ReminderFilter {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub label_id: Option<Uuid>,
    pub search: Option<String>,
    pub session_id: Option<Uuid>,
    pub limit: i64,
    pub offset: i64,
}

/// Body for PATCH /reminders/{id}.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchReminderRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub status: Option<String>,
    pub labels: Option<Vec<Uuid>>,
}

// ── Label ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, ToSchema)]
pub struct Label {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub color: String,
    pub bg_color: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateLabelRequest {
    pub name: String,
    pub color: String,
    pub bg_color: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PatchLabelRequest {
    pub name: Option<String>,
    pub color: Option<String>,
    pub bg_color: Option<String>,
}

// ── Session listing ───────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct ListSessionsParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
```

- [ ] **Step 2: Compile check**

```bash
cargo check
```

Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add src/domain/types.rs
git commit -m "feat: add Reminder, Label, and patch/filter domain types"
```

---

## Task 3: Repository — reminder.rs

**Files:**
- Create: `src/repository/reminder.rs`
- Modify: `src/repository/mod.rs`

- [ ] **Step 1: Write unit test for status transition logic**

The status transition logic will live in a helper function. Write the test first. Add at the end of the new `src/repository/reminder.rs` (create the file):

```rust
// src/repository/reminder.rs
use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::types::{NewReminder, PatchReminderRequest, Reminder, ReminderFilter, ReminderRow};

// ── helpers ──────────────────────────────────────────────────────────────

/// Compute the new archived_at value given a status transition.
/// Returns (new_status, new_archived_at_sql_fragment_is_now, clear_archived_at).
pub fn archived_at_for_status(old_status: &str, new_status: &str) -> Option<bool> {
    match (old_status, new_status) {
        (s, "archived") if s != "archived" => Some(true),  // set now()
        (_, "open") | (_, "completed") => Some(false),     // clear
        _ => None,                                          // no change
    }
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
```

- [ ] **Step 2: Run test to verify it fails (function not yet used, so just type-check)**

```bash
cargo test repository::reminder::tests -- --nocapture
```

Expected: 4 tests pass (the helper is pure Rust, no DB needed).

- [ ] **Step 3: Write the remaining repository functions**

Append to `src/repository/reminder.rs` (before the `#[cfg(test)]` block):

```rust
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
```

- [ ] **Step 4: Export reminder module**

Edit `src/repository/mod.rs` — add `pub mod reminder;`:

```rust
pub mod db;
pub mod reminder;
pub mod session;
pub mod speaker;
pub mod todo;
pub mod transcript;
```

- [ ] **Step 5: Compile check**

```bash
cargo check
```

Expected: compiles.

- [ ] **Step 6: Run unit tests**

```bash
cargo test repository::reminder::tests -- --nocapture
```

Expected: `open_to_archived_sets_timestamp ... ok`, `archived_to_open_clears_timestamp ... ok`, `open_to_completed_clears_timestamp ... ok`, `archived_to_archived_no_change ... ok` — 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/repository/reminder.rs src/repository/mod.rs
git commit -m "feat: reminder repository with CRUD, label join, and batch insert"
```

---

## Task 4: Repository — label.rs

**Files:**
- Create: `src/repository/label.rs`
- Modify: `src/repository/mod.rs`

- [ ] **Step 1: Create label.rs**

Create `src/repository/label.rs`:

```rust
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::types::{CreateLabelRequest, Label, PatchLabelRequest};

pub async fn list_labels(pool: &PgPool, tenant_id: Uuid) -> Result<Vec<Label>, sqlx::Error> {
    sqlx::query_as::<_, Label>(
        "SELECT * FROM labels WHERE tenant_id = $1 ORDER BY name ASC",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

pub async fn create_label(
    pool: &PgPool,
    tenant_id: Uuid,
    req: &CreateLabelRequest,
) -> Result<Label, sqlx::Error> {
    sqlx::query_as::<_, Label>(
        "INSERT INTO labels (tenant_id, name, color, bg_color) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(tenant_id)
    .bind(&req.name)
    .bind(&req.color)
    .bind(&req.bg_color)
    .fetch_one(pool)
    .await
}

pub async fn get_label(pool: &PgPool, id: Uuid) -> Result<Option<Label>, sqlx::Error> {
    sqlx::query_as::<_, Label>("SELECT * FROM labels WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn patch_label(
    pool: &PgPool,
    id: Uuid,
    req: &PatchLabelRequest,
) -> Result<Option<Label>, sqlx::Error> {
    sqlx::query_as::<_, Label>(
        r#"UPDATE labels SET
               name     = COALESCE($1, name),
               color    = COALESCE($2, color),
               bg_color = COALESCE($3, bg_color)
           WHERE id = $4
           RETURNING *"#,
    )
    .bind(&req.name)
    .bind(&req.color)
    .bind(&req.bg_color)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn delete_label(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM labels WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 2: Export label module**

Edit `src/repository/mod.rs`:

```rust
pub mod db;
pub mod label;
pub mod reminder;
pub mod session;
pub mod speaker;
pub mod todo;
pub mod transcript;
```

- [ ] **Step 3: Compile check**

```bash
cargo check
```

Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add src/repository/label.rs src/repository/mod.rs
git commit -m "feat: label repository CRUD"
```

---

## Task 5: Repository — session listing + speaker mutations

**Files:**
- Modify: `src/repository/session.rs`
- Modify: `src/repository/speaker.rs`

- [ ] **Step 1: Add list_sessions to session.rs**

Read `src/repository/session.rs`, then append:

```rust
pub async fn list_sessions(
    pool: &PgPool,
    tenant_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<Vec<AudioSession>, sqlx::Error> {
    sqlx::query_as::<_, AudioSession>(
        "SELECT * FROM audio_sessions WHERE tenant_id = $1 ORDER BY started_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(tenant_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
}
```

- [ ] **Step 2: Add update_speaker and soft_delete_speaker to speaker.rs**

Read `src/repository/speaker.rs`, then append:

```rust
pub async fn update_speaker(
    pool: &PgPool,
    id: Uuid,
    display_name: &str,
) -> Result<Option<Speaker>, sqlx::Error> {
    sqlx::query_as::<_, Speaker>(
        "UPDATE speakers SET display_name = $1 WHERE id = $2 RETURNING *",
    )
    .bind(display_name)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn soft_delete_speaker(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let result =
        sqlx::query("UPDATE speakers SET status = 'inactive' WHERE id = $1 AND status = 'active'")
            .bind(id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 3: Compile check**

```bash
cargo check
```

Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add src/repository/session.rs src/repository/speaker.rs
git commit -m "feat: add session listing and speaker update/soft-delete"
```

---

## Task 6: Update todo.rs and todo_generator.rs

**Files:**
- Modify: `src/repository/todo.rs`
- Modify: `src/engine/todo_generator.rs`
- Modify: `src/domain/types.rs` (remove `NewTodo` usage from generator)

The `todos` table was renamed to `reminders` in migration 007. Update `todo.rs` SQL and redirect `todo_generator.rs` to use `reminder_repo`.

- [ ] **Step 1: Update SQL table references in todo.rs**

Read `src/repository/todo.rs`. Change every occurrence of `FROM todos` / `INTO todos` to `FROM reminders` / `INTO reminders`. The full updated file:

```rust
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::types::TodoItem;
use crate::domain::types::NewTodo;

/// Check if any reminders already exist for a session (idempotency guard).
pub async fn has_todos(pool: &PgPool, session_id: Uuid) -> Result<bool, sqlx::Error> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM reminders WHERE session_id = $1)"
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Batch insert reminders — used by the backward-compat alias route.
pub async fn create_todos(
    pool: &PgPool,
    items: &[NewTodo],
) -> Result<Vec<TodoItem>, sqlx::Error> {
    if items.is_empty() {
        return Ok(vec![]);
    }

    let mut results = Vec::with_capacity(items.len());

    for item in items {
        let row: Option<TodoItem> = sqlx::query_as(
            "INSERT INTO reminders (session_id, speaker_id, assigned_to, description, priority) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (session_id, description) DO NOTHING \
             RETURNING *"
        )
        .bind(item.session_id)
        .bind(item.speaker_id)
        .bind(&item.assigned_to)
        .bind(&item.description)
        .bind(&item.priority)
        .fetch_optional(pool)
        .await?;

        if let Some(todo) = row {
            results.push(todo);
        }
    }

    Ok(results)
}

/// Get all reminders for a session — used by the backward-compat alias route.
pub async fn get_todos_for_session(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Vec<TodoItem>, sqlx::Error> {
    sqlx::query_as::<_, TodoItem>(
        "SELECT * FROM reminders WHERE session_id = $1 ORDER BY created_at ASC"
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
}
```

Note: `TodoItem` is fetched from the `reminders` table. The new columns (`title`, `due_time`, etc.) are nullable, so the existing `sqlx::FromRow` on `TodoItem` will need those fields added or the query limited. Because `TodoItem` uses `sqlx::FromRow` (which maps by column name), add the new nullable columns to `TodoItem` in `types.rs`. Read `src/domain/types.rs` and update `TodoItem`:

```rust
#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct TodoItem {
    pub id: Uuid,
    pub session_id: Uuid,
    pub speaker_id: Option<Uuid>,
    pub assigned_to: Option<String>,
    pub description: String,
    pub status: TodoStatus,
    pub priority: Option<TodoPriority>,
    // New columns from migration 007 — nullable, ignored by old callers
    pub tenant_id: Uuid,
    pub title: Option<String>,
    pub due_time: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub transcript_excerpt: Option<String>,
    pub context: Option<String>,
    pub source_time: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

- [ ] **Step 2: Update todo_generator.rs to use reminder_repo**

Replace the imports and calls in `src/engine/todo_generator.rs`. The full updated file:

```rust
use sqlx::PgPool;
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::engine::llm_client::LlmClient;
use crate::repository::{speaker as speaker_repo, reminder as reminder_repo, transcript};
use crate::domain::types::{NewReminder, Transcript};

pub const MAX_TRANSCRIPT_CHARS: usize = 50000;

pub async fn generate_session_todos(
    pool: &PgPool,
    llm_client: &LlmClient,
    session_id: Uuid,
    tenant_id: Uuid,
) -> Result<(), anyhow::Error> {
    info!(?session_id, "Generating reminders for session");

    if reminder_repo::has_reminders(pool, session_id).await? {
        info!(?session_id, "Reminders already exist for session, skipping");
        return Ok(());
    }

    let transcripts = transcript::get_final_transcripts_for_session(pool, session_id).await?;
    if transcripts.is_empty() {
        info!(?session_id, "No transcripts found, skipping reminder generation");
        return Ok(());
    }

    let transcript_text = build_transcript_string(&transcripts);
    info!(chars = transcript_text.len(), "Built transcript string");
    let transcript_text = truncate_transcript(&transcript_text);

    let llm_items = match llm_client.generate_todos(transcript_text).await {
        Ok(items) => items,
        Err(e) => {
            error!(error = %e, "LLM failed for reminder generation");
            return Err(e.into());
        }
    };

    if llm_items.is_empty() {
        info!(?session_id, "LLM returned no action items");
        return Ok(());
    }

    let mut new_reminders = Vec::new();
    for item in &llm_items {
        let speaker_id = if let Some(ref name) = item.speaker_name {
            match resolve_speaker_id(pool, tenant_id, name).await {
                Ok(id) => id,
                Err(e) => {
                    warn!(speaker_name = name, error = %e, "Failed to resolve speaker");
                    None
                }
            }
        } else {
            None
        };

        new_reminders.push(NewReminder {
            session_id: Some(session_id),
            tenant_id,
            speaker_id,
            assigned_to: item.assigned_to.clone(),
            title: None,
            description: item.description.clone(),
            priority: item.priority.clone(),
            transcript_excerpt: None,
            context: None,
            source_time: None,
        });
    }

    let inserted = reminder_repo::create_reminders_batch(pool, &new_reminders).await?;
    info!(count = inserted.len(), "Inserted reminders into database");

    Ok(())
}

pub fn build_transcript_string(transcripts: &[Transcript]) -> String {
    transcripts
        .iter()
        .map(|t| format!("[Unknown]: {}", t.text))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn truncate_transcript(text: &str) -> &str {
    if text.len() <= MAX_TRANSCRIPT_CHARS {
        return text;
    }
    let truncated = &text[..MAX_TRANSCRIPT_CHARS];
    if let Some(pos) = truncated.rfind("\n[") {
        return &text[..pos];
    }
    &text[..MAX_TRANSCRIPT_CHARS]
}

async fn resolve_speaker_id(
    pool: &PgPool,
    tenant_id: Uuid,
    speaker_name: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let speakers = speaker_repo::list_speakers(pool, tenant_id).await?;
    Ok(speakers
        .iter()
        .find(|s| s.display_name.eq_ignore_ascii_case(speaker_name))
        .map(|s| s.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_transcript_short_enough() {
        let text = "[Alice]: Hello\n[Bob]: Hi";
        assert_eq!(truncate_transcript(text), text);
    }

    #[test]
    fn test_truncate_at_boundary() {
        let mut text = String::new();
        for i in 0..6000 {
            text.push_str(&format!("[Speaker_{i}] This is some content.\n"));
        }
        let result = truncate_transcript(&text);
        assert!(result.len() <= MAX_TRANSCRIPT_CHARS);
        assert!(!result.ends_with("\n["));
    }

    #[test]
    fn test_build_transcript_empty() {
        let transcripts: Vec<Transcript> = vec![];
        assert!(build_transcript_string(&transcripts).is_empty());
    }

    #[test]
    fn test_build_transcript_single_item() {
        use chrono::Utc;
        use uuid::Uuid;
        let t = Transcript {
            id: Uuid::nil(),
            session_id: Uuid::nil(),
            segment_id: None,
            start_ms: 0,
            end_ms: 1000,
            text: "Hello world".to_string(),
            is_final: true,
            backend_type: "local".to_string(),
            created_at: Utc::now(),
        };
        assert_eq!(build_transcript_string(&[t]), "[Unknown]: Hello world");
    }
}
```

- [ ] **Step 3: Compile and run all unit tests**

```bash
cargo test -- --nocapture 2>&1 | grep -E "test result|FAILED|ok$"
```

Expected: all existing 14+ unit tests pass, plus the 4 new `repository::reminder::tests`.

- [ ] **Step 4: Commit**

```bash
git add src/repository/todo.rs src/engine/todo_generator.rs src/domain/types.rs
git commit -m "refactor: point todo.rs and todo_generator to renamed reminders table"
```

---

## Task 7: API — reminder.rs

**Files:**
- Create: `src/api/reminder.rs`

- [ ] **Step 1: Create reminder API handlers**

Create `src/api/reminder.rs`:

```rust
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::api::session::AppApiError;
use crate::domain::types::{CreateLabelRequest, NewReminder, PatchReminderRequest, Reminder, ReminderFilter};
use crate::repository::reminder as reminder_repo;
use crate::AppState;

fn tenant_id_from_headers(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Uuid::nil())
}

#[derive(Debug, Deserialize)]
pub struct ListRemindersQuery {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub label_id: Option<Uuid>,
    pub search: Option<String>,
    pub session_id: Option<Uuid>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_reminders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<ListRemindersQuery>,
) -> Result<Json<Vec<Reminder>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers);
    let filter = ReminderFilter {
        status: q.status,
        priority: q.priority,
        label_id: q.label_id,
        search: q.search,
        session_id: q.session_id,
        limit: q.limit.unwrap_or(50).min(200),
        offset: q.offset.unwrap_or(0),
    };
    let reminders = reminder_repo::list_reminders(&state.pool, tenant_id, &filter)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(reminders))
}

#[derive(Debug, Deserialize)]
pub struct CreateReminderRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub due_time: Option<chrono::DateTime<chrono::Utc>>,
    pub labels: Option<Vec<Uuid>>,
    pub session_id: Option<Uuid>,
}

pub async fn create_reminder(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateReminderRequest>,
) -> Result<(StatusCode, Json<Reminder>), AppApiError> {
    if req.title.is_none() && req.description.is_none() {
        return Err(AppApiError("title or description is required".into()));
    }
    let tenant_id = tenant_id_from_headers(&headers);
    let label_ids = req.labels.as_deref().unwrap_or(&[]);
    let new_reminder = NewReminder {
        session_id: req.session_id,
        tenant_id,
        speaker_id: None,
        assigned_to: None,
        title: req.title,
        description: req.description.unwrap_or_default(),
        priority: req.priority,
        transcript_excerpt: None,
        context: None,
        source_time: None,
    };
    let reminder = reminder_repo::create_reminder(&state.pool, &new_reminder, label_ids)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(reminder)))
}

pub async fn get_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Reminder>, AppApiError> {
    match reminder_repo::get_reminder(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?
    {
        Some(r) => Ok(Json(r)),
        None => Err(AppApiError("not found".into())),
    }
}

pub async fn patch_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<PatchReminderRequest>,
) -> Result<Json<Reminder>, AppApiError> {
    if let Some(ref s) = patch.status {
        if !["open", "completed", "archived"].contains(&s.as_str()) {
            return Err(AppApiError(format!("invalid status: {s}")));
        }
    }
    if let Some(ref p) = patch.priority {
        if !["high", "medium", "low"].contains(&p.as_str()) {
            return Err(AppApiError(format!("invalid priority: {p}")));
        }
    }
    match reminder_repo::patch_reminder(&state.pool, id, &patch)
        .await
        .map_err(|e| AppApiError(e.to_string()))?
    {
        Some(r) => Ok(Json(r)),
        None => Err(AppApiError("not found".into())),
    }
}

pub async fn delete_reminder(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = reminder_repo::delete_reminder(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError("not found".into()))
    }
}
```

- [ ] **Step 2: Compile check**

```bash
cargo check
```

Expected: compiles. (Routes not registered yet — that's Task 9.)

- [ ] **Step 3: Commit**

```bash
git add src/api/reminder.rs
git commit -m "feat: reminder API handlers (list, create, get, patch, delete)"
```

---

## Task 8: API — label.rs

**Files:**
- Create: `src/api/label.rs`

- [ ] **Step 1: Create label API handlers**

Create `src/api/label.rs`:

```rust
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use uuid::Uuid;

use crate::api::session::AppApiError;
use crate::domain::types::{CreateLabelRequest, Label, PatchLabelRequest};
use crate::repository::label as label_repo;
use crate::AppState;

fn tenant_id_from_headers(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(Uuid::nil())
}

pub async fn list_labels(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Label>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers);
    let labels = label_repo::list_labels(&state.pool, tenant_id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(labels))
}

pub async fn create_label(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateLabelRequest>,
) -> Result<(StatusCode, Json<Label>), AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers);
    match label_repo::create_label(&state.pool, tenant_id, &req).await {
        Ok(label) => Ok((StatusCode::CREATED, Json(label))),
        Err(sqlx::Error::Database(e)) if e.constraint() == Some("labels_tenant_id_name_key") => {
            Err(AppApiError(format!("label '{}' already exists", req.name)))
        }
        Err(e) => Err(AppApiError(e.to_string())),
    }
}

pub async fn patch_label(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchLabelRequest>,
) -> Result<Json<Label>, AppApiError> {
    match label_repo::patch_label(&state.pool, id, &req)
        .await
        .map_err(|e| AppApiError(e.to_string()))?
    {
        Some(l) => Ok(Json(l)),
        None => Err(AppApiError("not found".into())),
    }
}

pub async fn delete_label(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = label_repo::delete_label(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError("not found".into()))
    }
}
```

- [ ] **Step 2: Compile check**

```bash
cargo check
```

Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add src/api/label.rs
git commit -m "feat: label API handlers (list, create, patch, delete)"
```

---

## Task 9: API — session list + speaker mutations + register all routes

**Files:**
- Modify: `src/api/session.rs`
- Modify: `src/api/mod.rs`

- [ ] **Step 1: Add list_sessions handler and speaker handlers to session.rs**

Read `src/api/session.rs`. Add the following handlers. Insert before the `// --- Error ---` section:

```rust
// --- Session listing ---

#[utoipa::path(
    get,
    path = "/sessions",
    tag = "sessions",
    responses(
        (status = 200, description = "List of sessions", body = Vec<AudioSession>),
    ),
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<crate::domain::types::ListSessionsParams>,
) -> Result<Json<Vec<AudioSession>>, AppApiError> {
    let tenant_id = tenant_id_from_headers(&headers)?;
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);
    let sessions = session::list_sessions(&state.pool, tenant_id, limit, offset)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    Ok(Json(sessions))
}

// --- Speaker mutations ---

#[derive(Deserialize, ToSchema)]
pub struct UpdateSpeakerRequest {
    pub display_name: String,
}

#[utoipa::path(
    patch,
    path = "/speakers/{id}",
    tag = "speakers",
    params(("id" = Uuid, Path, description = "Speaker ID")),
    responses(
        (status = 200, description = "Updated speaker", body = Speaker),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn update_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSpeakerRequest>,
) -> Result<Json<Speaker>, AppApiError> {
    match speaker::update_speaker(&state.pool, id, &req.display_name)
        .await
        .map_err(|e| AppApiError(e.to_string()))?
    {
        Some(s) => Ok(Json(s)),
        None => Err(AppApiError("not found".into())),
    }
}

#[utoipa::path(
    delete,
    path = "/speakers/{id}",
    tag = "speakers",
    params(("id" = Uuid, Path, description = "Speaker ID")),
    responses(
        (status = 204, description = "Speaker soft-deleted"),
        (status = 500, description = "Internal server error", body = AppApiError),
    ),
)]
pub async fn delete_speaker(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppApiError> {
    let deleted = speaker::soft_delete_speaker(&state.pool, id)
        .await
        .map_err(|e| AppApiError(e.to_string()))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppApiError("not found".into()))
    }
}
```

Also add `use axum::extract::Query;` to the imports at the top of session.rs (alongside the existing `use axum::extract::{Path, State};`):

```rust
use axum::extract::{Path, Query, State};
```

- [ ] **Step 2: Update mod.rs with all new routes and OpenAPI schemas**

Read `src/api/mod.rs`. Replace the entire file with:

```rust
pub mod label;
pub mod reminder;
pub mod session;
pub mod ws;

use axum::routing::{delete, get, patch, post};
use axum::Router;
use axum::Json;
use axum::extract::State;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::AppState;
use crate::api::session::*;
use crate::domain::types::*;
use crate::engine::metrics::HealthSummary;
use std::sync::atomic::Ordering;

#[derive(OpenApi)]
#[openapi(
    info(title = "Actio ASR API", version = "0.1.0"),
    paths(
        create_session,
        list_sessions,
        get_session,
        end_session,
        get_transcripts,
        get_todo_items,
        create_speaker,
        list_speakers,
        update_speaker,
        delete_speaker,
    ),
    components(schemas(
        CreateSessionRequest,
        SessionResponse,
        CreateSpeakerRequest,
        UpdateSpeakerRequest,
        AudioSession,
        Speaker,
        Transcript,
        TodoItem,
        TodoStatus,
        TodoPriority,
        TodoListResponse,
        Reminder,
        Label,
        CreateLabelRequest,
        PatchLabelRequest,
        PatchReminderRequest,
        AppApiError,
    )),
)]
struct ApiDoc;

pub fn router(state: AppState) -> Router {
    Router::new()
        // health
        .route("/health", get(health))
        // sessions
        .route("/sessions", get(session::list_sessions))
        .route("/sessions", post(session::create_session))
        .route("/sessions/{id}", get(session::get_session))
        .route("/sessions/{id}/end", post(session::end_session))
        .route("/sessions/{id}/transcripts", get(session::get_transcripts))
        .route("/sessions/{id}/todos", get(session::get_todo_items))
        // reminders
        .route("/reminders", get(reminder::list_reminders))
        .route("/reminders", post(reminder::create_reminder))
        .route("/reminders/{id}", get(reminder::get_reminder))
        .route("/reminders/{id}", patch(reminder::patch_reminder))
        .route("/reminders/{id}", delete(reminder::delete_reminder))
        // labels
        .route("/labels", get(label::list_labels))
        .route("/labels", post(label::create_label))
        .route("/labels/{id}", patch(label::patch_label))
        .route("/labels/{id}", delete(label::delete_label))
        // speakers
        .route("/speakers", post(session::create_speaker))
        .route("/speakers", get(session::list_speakers))
        .route("/speakers/{id}", patch(session::update_speaker))
        .route("/speakers/{id}", delete(session::delete_speaker))
        // docs
        .route("/api-docs/openapi.json", get(openapi))
        .route("/ws", get(ws::ws_session))
        .with_state(state)
        .merge(SwaggerUi::new("/docs"))
}

async fn health(State(state): State<AppState>) -> Json<HealthSummary> {
    let worker_state = if state.inference_router.is_some() {
        "available"
    } else {
        "degraded"
    }
    .to_string();

    Json(HealthSummary {
        active_sessions: state.metrics.active_sessions.load(Ordering::Relaxed),
        uptime_secs: state.metrics.uptime_secs(),
        worker_state,
        local_route_count: state.metrics.local_route_count.load(Ordering::Relaxed),
        worker_error_count: state.metrics.worker_error_count.load(Ordering::Relaxed),
        unknown_speaker_count: state.metrics.unknown_speaker_count.load(Ordering::Relaxed),
    })
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}
```

- [ ] **Step 3: Compile check**

```bash
cargo check
```

Expected: compiles. Fix any import errors (e.g., missing `use` statements) as needed — the error messages will point to exact lines.

- [ ] **Step 4: Run all tests**

```bash
cargo test -- --nocapture 2>&1 | grep -E "test result|FAILED|ok$"
```

Expected: all existing + new unit tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/api/session.rs src/api/mod.rs
git commit -m "feat: register reminder/label/speaker routes and OpenAPI schemas"
```

---

## Task 10: Test Coverage

**Files:**
- Modify: `tests/test_e2e_session.rs`

Add tests for the new endpoints using the in-process axum router pattern already established in `tests/test_e2e_session.rs`.

- [ ] **Step 1: Write test for label CRUD**

Read `tests/test_e2e_session.rs`. Append the following test:

```rust
#[tokio::test]
async fn label_crud_create_list_patch_delete() {
    let pool = common::test_pool().await;
    let deps = common::test_app_deps(&pool);

    let app = api::router(AppState {
        pool: pool.clone(),
        coordinator: deps.coordinator,
        aggregator: deps.aggregator,
        circuit_breaker: deps.circuit_breaker,
        inference_router: None,
        metrics: deps.metrics,
        llm_client: None,
    });

    // POST /labels
    let create_resp = app
        .clone()
        .oneshot(
            Request::post("/labels")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": "TestLabel",
                        "color": "#123456",
                        "bg_color": "#abcdef"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
    let label: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let label_id: uuid::Uuid = label["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(label["name"], "TestLabel");

    // GET /labels
    let list_resp = app
        .clone()
        .oneshot(Request::get("/labels").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_resp.into_body(), usize::MAX).await.unwrap();
    let labels: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    assert!(labels.as_array().unwrap().iter().any(|l| l["id"] == label["id"]));

    // PATCH /labels/{id}
    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/labels/{label_id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"name": "Renamed"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);

    // DELETE /labels/{id}
    let del_resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/labels/{label_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
}
```

- [ ] **Step 2: Write test for reminder create + patch status**

Append to `tests/test_e2e_session.rs`:

```rust
#[tokio::test]
async fn reminder_create_and_patch_status() {
    let pool = common::test_pool().await;
    let deps = common::test_app_deps(&pool);

    let app = api::router(AppState {
        pool: pool.clone(),
        coordinator: deps.coordinator,
        aggregator: deps.aggregator,
        circuit_breaker: deps.circuit_breaker,
        inference_router: None,
        metrics: deps.metrics,
        llm_client: None,
    });

    // POST /reminders
    let create_resp = app
        .clone()
        .oneshot(
            Request::post("/reminders")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "title": "Buy milk",
                        "priority": "low"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
    let reminder: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let reminder_id: uuid::Uuid = reminder["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(reminder["status"], "open");
    assert_eq!(reminder["labels"].as_array().unwrap().len(), 0);

    // PATCH /reminders/{id} — mark archived
    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/reminders/{reminder_id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"status": "archived"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let patch_body = axum::body::to_bytes(patch_resp.into_body(), usize::MAX).await.unwrap();
    let patched: serde_json::Value = serde_json::from_slice(&patch_body).unwrap();
    assert_eq!(patched["status"], "archived");
    assert!(patched["archived_at"].as_str().is_some());

    // DELETE /reminders/{id}
    let del_resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/reminders/{reminder_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
}
```

- [ ] **Step 3: Run unit tests (no DB required)**

```bash
cargo test -- --nocapture 2>&1 | grep -E "test result|FAILED"
```

Expected: all unit tests pass. The new e2e tests will be skipped/fail without DATABASE_URL — that's expected.

- [ ] **Step 4: Run e2e tests with a real DB (optional, requires DATABASE_URL)**

```bash
# With Docker running:
# docker compose up -d (from backend/)
export DATABASE_URL=postgres://actio:actio@localhost:5433/actio
cargo test --test test_e2e_session -- --nocapture
```

Expected: all 3 e2e tests pass.

- [ ] **Step 5: Commit**

```bash
git add tests/test_e2e_session.rs
git commit -m "test: add label CRUD and reminder create/patch/archive e2e tests"
```

---

## Execution Order

```
Task 1 (migrations) → Task 2 (types) → Task 3 (reminder repo) → Task 4 (label repo)
  → Task 5 (session + speaker repo) → Task 6 (todo.rs + generator)
  → Task 7 (reminder API) → Task 8 (label API)
  → Task 9 (routes + session/speaker API) → Task 10 (tests)
```

Tasks 7 and 8 can run in parallel. Tasks 3, 4, and 5 can run in parallel after Task 2.

---

## Completion Checklist

- [ ] `cargo check` passes
- [ ] `cargo test` passes (14+ unit tests)
- [ ] `GET /reminders` returns filtered list with `labels: []` or populated
- [ ] `POST /reminders` creates a reminder with `status: "open"`
- [ ] `PATCH /reminders/{id}` with `status: "archived"` sets `archived_at`
- [ ] `PATCH /reminders/{id}` with `status: "open"` clears `archived_at`
- [ ] `DELETE /reminders/{id}` returns 204
- [ ] `GET /labels` returns labels for tenant
- [ ] `POST /labels` returns 201; second call with same name returns 409
- [ ] `DELETE /labels/{id}` cascades through reminder_labels
- [ ] `GET /sessions` returns sessions newest-first
- [ ] `PATCH /speakers/{id}` updates display_name
- [ ] `DELETE /speakers/{id}` sets status = inactive; excluded from `GET /speakers`
- [ ] `GET /sessions/{id}/todos` still works (backward-compat alias)
- [ ] `cargo clippy` passes with no new warnings
