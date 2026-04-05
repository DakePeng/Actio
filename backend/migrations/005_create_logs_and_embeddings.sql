CREATE TABLE speaker_embeddings (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    speaker_id UUID NOT NULL REFERENCES speakers(id) ON DELETE CASCADE,
    model_name TEXT NOT NULL DEFAULT 'CAM++',
    model_version TEXT NOT NULL DEFAULT '1.0',
    embedding vector(192) NOT NULL,
    duration_ms FLOAT NOT NULL,
    quality_score FLOAT,
    is_primary BOOLEAN NOT NULL DEFAULT false,
    embedding_dimension INT NOT NULL DEFAULT 192,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_embeddings_speaker ON speaker_embeddings(speaker_id);
CREATE INDEX idx_embeddings_vector ON speaker_embeddings
    USING hnsw (embedding vector_cosine_ops) WITH (m = 16, ef_construction = 64);

CREATE UNIQUE INDEX idx_embeddings_primary ON speaker_embeddings(speaker_id)
    WHERE is_primary = true;

CREATE TABLE verification_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    session_id UUID,
    segment_id UUID,
    target_speaker_id UUID,
    score FLOAT,
    threshold FLOAT,
    decision TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE routing_decision_logs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    session_id UUID,
    decision TEXT NOT NULL,
    reason TEXT,
    latency_ms FLOAT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
