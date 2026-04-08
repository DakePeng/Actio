# Reminders API — Design Spec

**Date:** 2026-04-08  
**Status:** Approved  
**Scope:** Backend only — frontend wiring is a separate piece of work

---

## Problem

The frontend board UI uses a rich `Reminder` type (`title`, `dueTime`, `labels`, `transcript`, `context`, `sourceTime`, `archivedAt`) and manages all state in-memory via Zustand. The backend `Todo` type is a thin LLM-extraction record with none of these fields. Labels have no backend representation at all. The two sides are entirely disconnected.

---

## Decision

Extend the existing `todos` table into a full `reminders` entity (Option A — merge). Rename the table and API paths to `reminders`. Add a `labels` table with a `reminder_labels` junction. Add missing CRUD endpoints throughout. No new concepts are introduced — the LLM extraction flow continues to write into the same table.

---

## Schema Changes

### Migration 007 — Rename todos → reminders, add columns

```sql
ALTER TABLE todos RENAME TO reminders;

ALTER TABLE reminders
  ADD COLUMN title              TEXT,
  ADD COLUMN due_time           TIMESTAMPTZ,
  ADD COLUMN archived_at        TIMESTAMPTZ,
  ADD COLUMN transcript_excerpt TEXT,
  ADD COLUMN context            TEXT,
  ADD COLUMN source_time        TIMESTAMPTZ;
```

**Column notes:**
- `title` — nullable; LLM-generated reminders may only have `description` initially
- `due_time` — optional deadline, displayed in the board card
- `archived_at` — timestamp set when `status` transitions to `'archived'`; `status = 'archived'` already exists on the column
- `transcript_excerpt` — short quote from the source transcript shown in the card detail view
- `context` — human-readable provenance string e.g. "Extracted from voice note at 9:42 AM"
- `source_time` — timestamp of the audio segment the reminder was extracted from

Existing columns retained unchanged: `id`, `session_id`, `speaker_id`, `assigned_to`, `description`, `status`, `priority`, `created_at`, `updated_at`.

### Migration 008 — Labels and reminder_labels

```sql
CREATE TABLE labels (
  id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id  UUID        NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
  name       TEXT        NOT NULL,
  color      VARCHAR(7)  NOT NULL,   -- hex foreground e.g. '#6366F1'
  bg_color   VARCHAR(7)  NOT NULL,   -- hex background e.g. '#EEF2FF'
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, name)
);

CREATE TABLE reminder_labels (
  reminder_id UUID NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
  label_id    UUID NOT NULL REFERENCES labels(id)    ON DELETE CASCADE,
  PRIMARY KEY (reminder_id, label_id)
);

CREATE INDEX idx_labels_tenant ON labels(tenant_id);
CREATE INDEX idx_reminder_labels_label ON reminder_labels(label_id);
```

### Migration 009 — Session listing index

```sql
CREATE INDEX idx_sessions_tenant_started
  ON audio_sessions(tenant_id, started_at DESC);
```

---

## API Endpoints

All endpoints use `x-tenant-id` header (falls back to nil UUID, matching existing convention).

### Reminders

#### `GET /reminders`

List reminders for the tenant. Supports filtering and pagination.

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| `status` | `open\|completed\|archived` | Filter by status (default: `open`) |
| `priority` | `high\|medium\|low` | Filter by priority |
| `label_id` | UUID | Filter to reminders that have this label |
| `search` | string | Full-text match on `title` + `description` (case-insensitive `ILIKE`) |
| `session_id` | UUID | Filter to reminders from a specific session |
| `limit` | int | Max results, default 50, max 200 |
| `offset` | int | Pagination offset, default 0 |

**Response `200`**
```json
[
  {
    "id": "...",
    "session_id": "...",
    "speaker_id": null,
    "assigned_to": "Alice",
    "title": "Prepare Q3 report",
    "description": "Finance numbers needed by Friday.",
    "priority": "high",
    "status": "open",
    "due_time": "2026-04-12T17:00:00Z",
    "archived_at": null,
    "transcript_excerpt": "Sarah needs the marketing numbers before the board meeting.",
    "context": "Extracted from voice note at 9:42 AM",
    "source_time": "2026-04-07T09:42:00Z",
    "labels": ["label-uuid-1", "label-uuid-2"],
    "created_at": "2026-04-08T10:00:00Z",
    "updated_at": "2026-04-08T10:00:00Z"
  }
]
```

`labels` is an array of label UUIDs joined from `reminder_labels`.

---

#### `POST /reminders`

Create a reminder manually (not from LLM).

**Request body**
```json
{
  "title": "Call dentist",
  "description": "Follow-up from last week.",
  "priority": "medium",
  "due_time": "2026-04-09T09:00:00Z",
  "labels": ["label-uuid-1"],
  "session_id": null
}
```

All fields optional except at least one of `title` or `description` must be present. `labels` defaults to `[]`.

**Response `201`** — full reminder object (same shape as GET item)

---

#### `GET /reminders/{id}`

Returns a single reminder with labels array. `404` if not found.

---

#### `PATCH /reminders/{id}`

Partial update. Only fields present in the request body are changed.

**Request body** — any subset of:
```json
{
  "title": "Updated title",
  "description": "Updated description",
  "priority": "low",
  "due_time": "2026-04-15T12:00:00Z",
  "status": "completed",
  "labels": ["label-uuid-2"]
}
```

**Status transition rules:**
- `open → completed` — no timestamp side-effect
- `open/completed → archived` — sets `archived_at = now()`
- `archived → open` — clears `archived_at`

**Labels replacement:** `labels` field replaces the full label set atomically (delete all `reminder_labels` for this reminder, re-insert new list) in a single transaction.

**Response `200`** — updated reminder object

---

#### `DELETE /reminders/{id}`

Hard delete. Cascades through `reminder_labels`. Returns `204`.

---

#### `GET /sessions/{id}/todos` (backward-compat alias)

Delegates to `GET /reminders?session_id={id}&status=` (no status filter — returns all). Returns same shape as before (`{ todos: [...], generated: true }`) for backward compatibility.

---

### Labels

#### `GET /labels`

Returns all labels for the tenant ordered by name.

**Response `200`**
```json
[
  {
    "id": "...",
    "tenant_id": "...",
    "name": "Work",
    "color": "#6366F1",
    "bg_color": "#EEF2FF",
    "created_at": "2026-04-08T10:00:00Z"
  }
]
```

---

#### `POST /labels`

**Request body**
```json
{ "name": "Work", "color": "#6366F1", "bg_color": "#EEF2FF" }
```

Returns `201` with created label. Returns `409` if a label with the same name already exists for this tenant.

---

#### `PATCH /labels/{id}`

Partial update of `name`, `color`, `bg_color`. Returns `200` with updated label.

---

#### `DELETE /labels/{id}`

Deletes the label and cascades through `reminder_labels` (removes it from all reminders). Returns `204`.

---

### Sessions

#### `GET /sessions`

List sessions for the tenant, newest first.

**Query parameters:** `limit` (default 20, max 100), `offset` (default 0).

**Response `200`** — array of `AudioSession` objects (existing type, no changes).

---

### Speakers

#### `PATCH /speakers/{id}`

**Request body**
```json
{ "display_name": "Alice" }
```

Returns `200` with updated speaker.

---

#### `DELETE /speakers/{id}`

Soft-delete: sets `status = 'inactive'`. Speaker is excluded from future `list_speakers` results (which already filter `status = 'active'`). Returns `204`.

---

## Rust Implementation

### New / changed files

| File | Change |
|------|--------|
| `migrations/007_rename_todos_to_reminders.sql` | New |
| `migrations/008_create_labels.sql` | New |
| `migrations/009_create_sessions_index.sql` | New |
| `src/domain/types.rs` | Add `Reminder`, `Label`; keep `TodoItem` as alias |
| `src/repository/reminder.rs` | New — CRUD with label join |
| `src/repository/label.rs` | New — label + reminder_labels CRUD |
| `src/repository/session.rs` | Add `list_sessions` |
| `src/repository/speaker.rs` | Add `update_speaker`, `soft_delete_speaker` |
| `src/repository/todo.rs` | Update SQL table references `todos` → `reminders`; keep `has_todos` + `get_todos_for_session` as compat functions |
| `src/engine/todo_generator.rs` | Update imports: use `reminder_repo::create_reminders_batch` instead of `todo_repo::create_todos` |
| `src/api/reminder.rs` | New — reminder handlers |
| `src/api/label.rs` | New — label handlers |
| `src/api/session.rs` | Add `list_sessions` handler; update `get_todo_items` alias |
| `src/api/mod.rs` | Register new routes |

### PATCH implementation note

Each PATCH handler uses an explicit patch struct with `Option<T>` fields. Only `Some` fields are written. For `labels`, a `Some(vec)` triggers a transactional replace (delete + re-insert). A missing `labels` key leaves labels unchanged.

### No Python worker changes

All changes are in the Rust data/API layer. The LLM extraction flow (`todo_generator.rs`) continues unchanged — it will write to the renamed `reminders` table via an updated `repository/reminder.rs`.

---

## Out of Scope

- Frontend API client wiring (separate spec)
- Speaker embedding enrollment endpoint (separate spec)
- Pagination cursor (offset is sufficient for MVP)
- Full-text search index (ILIKE is sufficient for MVP data volumes)
- Multi-tenant auth (existing nil-UUID convention preserved)
