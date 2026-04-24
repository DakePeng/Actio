-- Migration 004: always-listening action extraction windows.
--
-- Background: the legacy todo_generator runs once at session end. This is
-- useless for an always-on pipeline because the session never ends. We
-- replace it with a windowed extractor that runs every few minutes over
-- overlapping 5-minute windows of transcript+segment data. Each window
-- yields zero or more reminders, attributed to the exact speakers and
-- transcripts that produced them. Low-confidence LLM items are dropped;
-- medium-confidence items are parked with status='pending' for user review.

-- 1) Windows table. Identity is (session_id, start_ms) — one row per
-- window-of-a-session. status cycles pending → running → succeeded|empty|failed.
CREATE TABLE extraction_windows (
    id            TEXT PRIMARY KEY NOT NULL,
    session_id    TEXT NOT NULL REFERENCES audio_sessions(id) ON DELETE CASCADE,
    start_ms      INTEGER NOT NULL,
    end_ms        INTEGER NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending','running','succeeded','empty','failed')),
    attempts      INTEGER NOT NULL DEFAULT 0,
    last_error    TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at   TEXT,
    UNIQUE (session_id, start_ms)
);
CREATE INDEX idx_extraction_windows_status ON extraction_windows(status);
CREATE INDEX idx_extraction_windows_session ON extraction_windows(session_id);

-- 2) Rebuild `reminders` to:
--    (a) admit a new status value 'pending' (review queue for medium-
--        confidence items). SQLite can't ALTER a CHECK constraint.
--    (b) add `source_window_id` column so each auto-extracted reminder
--        knows which window produced it (trace / dedup / debugging).
-- Standard SQLite rebuild: temp table, copy, drop, rename, recreate indexes.

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
    source_window_id    TEXT REFERENCES extraction_windows(id) ON DELETE SET NULL,
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
    source_time, NULL, created_at, updated_at
FROM reminders;

DROP TABLE reminders;
ALTER TABLE reminders_new RENAME TO reminders;

CREATE INDEX idx_reminders_tenant ON reminders(tenant_id);
CREATE INDEX idx_reminders_status ON reminders(tenant_id, status);
CREATE INDEX idx_reminders_source_window ON reminders(source_window_id);

PRAGMA foreign_keys = ON;
