-- Migration 002: speaker diarization support
-- Adds: color column on speakers, embedding storage on audio_segments.

ALTER TABLE speakers
    ADD COLUMN color TEXT NOT NULL DEFAULT '#64B5F6';

ALTER TABLE audio_segments
    ADD COLUMN embedding BLOB;
ALTER TABLE audio_segments
    ADD COLUMN embedding_dim INTEGER;

CREATE INDEX IF NOT EXISTS idx_segments_unknown
    ON audio_segments(session_id, speaker_id)
    WHERE speaker_id IS NULL;
