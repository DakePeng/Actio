CREATE TABLE audio_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id UUID NOT NULL DEFAULT '00000000-0000-0000-0000-000000000000',
    source_type TEXT NOT NULL DEFAULT 'microphone',
    mode TEXT NOT NULL DEFAULT 'realtime',
    routing_policy TEXT NOT NULL DEFAULT 'local_first',
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ended_at TIMESTAMPTZ,
    metadata JSONB DEFAULT '{}'
);

CREATE INDEX idx_sessions_tenant ON audio_sessions(tenant_id);
CREATE INDEX idx_sessions_started ON audio_sessions(started_at);
