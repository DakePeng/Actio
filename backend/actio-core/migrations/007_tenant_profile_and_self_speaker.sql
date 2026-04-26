-- 007_tenant_profile_and_self_speaker.sql
-- Per-tenant identity (display name, multilingual aliases, free-form bio)
-- and a self-speaker flag used to ground the action-item extraction prompt.

CREATE TABLE IF NOT EXISTS tenant_profile (
    tenant_id     TEXT PRIMARY KEY,
    display_name  TEXT,
    aliases       TEXT NOT NULL DEFAULT '[]'
                  CHECK (json_valid(aliases) AND json_type(aliases) = 'array'),
    bio           TEXT,
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

ALTER TABLE speakers ADD COLUMN is_self INTEGER NOT NULL DEFAULT 0
    CHECK (is_self IN (0, 1));

-- Partial unique index: at most one is_self=1 row per tenant.
CREATE UNIQUE INDEX IF NOT EXISTS idx_speakers_one_self_per_tenant
    ON speakers(tenant_id) WHERE is_self = 1;
