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
