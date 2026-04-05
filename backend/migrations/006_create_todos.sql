CREATE TABLE IF NOT EXISTS todos (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    session_id UUID NOT NULL REFERENCES audio_sessions(id) ON DELETE CASCADE,
    speaker_id UUID REFERENCES speakers(id) ON DELETE SET NULL,
    assigned_to VARCHAR(255),
    description TEXT NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'completed', 'archived')),
    priority VARCHAR(20) CHECK (priority IN ('high', 'medium', 'low')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(session_id, description)
);

CREATE INDEX idx_todos_session ON todos(session_id);
