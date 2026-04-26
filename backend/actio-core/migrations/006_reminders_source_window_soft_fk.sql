-- Migration 006: drop the foreign-key constraint on reminders.source_window_id.
--
-- Migration 004 declared `source_window_id TEXT REFERENCES extraction_windows(id)
-- ON DELETE SET NULL` because windows were the only source of auto-extracted
-- reminders. Plan Task 10 (batch clip processing) repurposes this column to
-- reference `audio_clips.id` for new rows while legacy rows keep pointing at
-- extraction_windows. The trace endpoint resolves either pool. SQLite cannot
-- ALTER an existing FK in place, so we rebuild the table without the FK and
-- recreate every index. The column becomes a soft reference — orphaning is
-- the lookup endpoint's problem to handle, not the schema's.

PRAGMA foreign_keys = OFF;

CREATE TABLE reminders_new (
    id                  TEXT PRIMARY KEY NOT NULL,
    session_id          TEXT REFERENCES audio_sessions(id) ON DELETE CASCADE,
    tenant_id           TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    speaker_id          TEXT REFERENCES speakers(id) ON DELETE SET NULL,
    assigned_to         TEXT,
    title               TEXT,
    description         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'open'
                          CHECK (status IN ('open', 'pending', 'completed', 'archived')),
    priority            TEXT CHECK (priority IN ('high', 'medium', 'low')),
    due_time            TEXT,
    archived_at         TEXT,
    transcript_excerpt  TEXT,
    context             TEXT,
    source_time         TEXT,
    source_window_id    TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(session_id, description)
);

INSERT INTO reminders_new (
    id, session_id, tenant_id, speaker_id, assigned_to, title, description,
    status, priority, due_time, archived_at, transcript_excerpt, context,
    source_time, source_window_id, created_at, updated_at
)
SELECT
    id, session_id, tenant_id, speaker_id, assigned_to, title, description,
    status, priority, due_time, archived_at, transcript_excerpt, context,
    source_time, source_window_id, created_at, updated_at
FROM reminders;

DROP TABLE reminders;
ALTER TABLE reminders_new RENAME TO reminders;

CREATE INDEX idx_reminders_tenant ON reminders(tenant_id);
CREATE INDEX idx_reminders_status ON reminders(tenant_id, status);
CREATE INDEX idx_reminders_source_window ON reminders(source_window_id);

PRAGMA foreign_keys = ON;
