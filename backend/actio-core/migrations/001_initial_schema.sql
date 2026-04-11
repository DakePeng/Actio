-- Actio initial schema for SQLite
-- Consolidated from 10 PostgreSQL migrations into a single SQLite-compatible schema.
-- UUIDs are stored as TEXT (generated in Rust via uuid::Uuid::new_v4().to_string()).
-- Timestamps are stored as TEXT in ISO 8601 format (handled natively by sqlx + chrono).
-- Embeddings are stored as BLOB (raw f32 little-endian bytes, 512 dims = 2048 bytes).

-- speakers
CREATE TABLE IF NOT EXISTS speakers (
    id          TEXT PRIMARY KEY NOT NULL,
    tenant_id   TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    display_name TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_speakers_tenant ON speakers(tenant_id);

-- audio_sessions
CREATE TABLE IF NOT EXISTS audio_sessions (
    id              TEXT PRIMARY KEY NOT NULL,
    tenant_id       TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    source_type     TEXT NOT NULL DEFAULT 'microphone',
    mode            TEXT NOT NULL DEFAULT 'realtime',
    routing_policy  TEXT NOT NULL DEFAULT 'local_first',
    started_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ended_at        TEXT,
    metadata        TEXT DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_sessions_tenant ON audio_sessions(tenant_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON audio_sessions(started_at);
CREATE INDEX IF NOT EXISTS idx_sessions_tenant_started ON audio_sessions(tenant_id, started_at DESC);

-- audio_segments
CREATE TABLE IF NOT EXISTS audio_segments (
    id              TEXT PRIMARY KEY NOT NULL,
    session_id      TEXT NOT NULL REFERENCES audio_sessions(id) ON DELETE CASCADE,
    start_ms        INTEGER NOT NULL,
    end_ms          INTEGER NOT NULL,
    speaker_id      TEXT REFERENCES speakers(id),
    speaker_score   REAL,
    audio_ref       TEXT,
    quality_score   REAL,
    vad_confidence  REAL
);
CREATE INDEX IF NOT EXISTS idx_segments_session ON audio_segments(session_id);
CREATE INDEX IF NOT EXISTS idx_segments_speaker ON audio_segments(speaker_id);

-- transcripts
CREATE TABLE IF NOT EXISTS transcripts (
    id              TEXT PRIMARY KEY NOT NULL,
    session_id      TEXT NOT NULL REFERENCES audio_sessions(id) ON DELETE CASCADE,
    segment_id      TEXT REFERENCES audio_segments(id),
    start_ms        INTEGER NOT NULL,
    end_ms          INTEGER NOT NULL,
    text            TEXT NOT NULL,
    is_final        INTEGER NOT NULL DEFAULT 0,
    backend_type    TEXT NOT NULL DEFAULT 'local',
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_transcripts_session ON transcripts(session_id);
CREATE INDEX IF NOT EXISTS idx_transcripts_segment ON transcripts(segment_id);

-- reminders (consolidated from todos + rename + column additions)
CREATE TABLE IF NOT EXISTS reminders (
    id                  TEXT PRIMARY KEY NOT NULL,
    session_id          TEXT REFERENCES audio_sessions(id) ON DELETE CASCADE,
    tenant_id           TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    speaker_id          TEXT REFERENCES speakers(id) ON DELETE SET NULL,
    assigned_to         TEXT,
    title               TEXT,
    description         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'completed', 'archived')),
    priority            TEXT CHECK (priority IN ('high', 'medium', 'low')),
    due_time            TEXT,
    archived_at         TEXT,
    transcript_excerpt  TEXT,
    context             TEXT,
    source_time         TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(session_id, description)
);
CREATE INDEX IF NOT EXISTS idx_reminders_tenant ON reminders(tenant_id);
CREATE INDEX IF NOT EXISTS idx_reminders_status ON reminders(tenant_id, status);

-- labels
CREATE TABLE IF NOT EXISTS labels (
    id          TEXT PRIMARY KEY NOT NULL,
    tenant_id   TEXT NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    name        TEXT NOT NULL,
    color       TEXT NOT NULL,
    bg_color    TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (tenant_id, name)
);
CREATE INDEX IF NOT EXISTS idx_labels_tenant ON labels(tenant_id);

-- reminder_labels (join table)
CREATE TABLE IF NOT EXISTS reminder_labels (
    reminder_id TEXT NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
    label_id    TEXT NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    PRIMARY KEY (reminder_id, label_id)
);
CREATE INDEX IF NOT EXISTS idx_reminder_labels_label ON reminder_labels(label_id);

-- speaker_embeddings
CREATE TABLE IF NOT EXISTS speaker_embeddings (
    id                  TEXT PRIMARY KEY NOT NULL,
    speaker_id          TEXT NOT NULL REFERENCES speakers(id) ON DELETE CASCADE,
    model_name          TEXT NOT NULL DEFAULT 'ERes2Net',
    model_version       TEXT NOT NULL DEFAULT '1.0',
    embedding           BLOB NOT NULL,
    duration_ms         REAL NOT NULL,
    quality_score       REAL,
    is_primary          INTEGER NOT NULL DEFAULT 0,
    embedding_dimension INTEGER NOT NULL DEFAULT 512,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_embeddings_speaker ON speaker_embeddings(speaker_id);

-- verification_logs
CREATE TABLE IF NOT EXISTS verification_logs (
    id                  TEXT PRIMARY KEY NOT NULL,
    session_id          TEXT,
    segment_id          TEXT,
    target_speaker_id   TEXT,
    score               REAL,
    threshold           REAL,
    decision            TEXT NOT NULL,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- routing_decision_logs
CREATE TABLE IF NOT EXISTS routing_decision_logs (
    id          TEXT PRIMARY KEY NOT NULL,
    session_id  TEXT,
    decision    TEXT NOT NULL,
    reason      TEXT,
    latency_ms  REAL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
