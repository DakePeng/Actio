CREATE TABLE audio_segments (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    session_id UUID NOT NULL REFERENCES audio_sessions(id) ON DELETE CASCADE,
    start_ms BIGINT NOT NULL,
    end_ms BIGINT NOT NULL,
    speaker_id UUID REFERENCES speakers(id),
    speaker_score FLOAT,
    audio_ref TEXT,
    quality_score FLOAT,
    vad_confidence FLOAT
);

CREATE INDEX idx_segments_session ON audio_segments(session_id);
CREATE INDEX idx_segments_speaker ON audio_segments(speaker_id);
