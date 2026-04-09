CREATE INDEX idx_sessions_tenant_started
    ON audio_sessions(tenant_id, started_at DESC);
