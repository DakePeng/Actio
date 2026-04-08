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
