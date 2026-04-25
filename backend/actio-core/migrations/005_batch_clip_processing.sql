-- Migration 005: batch clip processing pipeline.
--
-- Replaces the streaming-derived persistence model. Each ~5-min audio clip
-- is now an explicit row with a manifest pointing at its on-disk per-VAD
-- segment WAVs. Transcripts and speaker assignments come from a deferred
-- batch pass over the clip; per-clip global clustering produces stable
-- attribution without requiring enrollment.

-- 1) Clips table. status cycles pending → running → processed | empty | failed.
CREATE TABLE audio_clips (
    id              TEXT PRIMARY KEY NOT NULL,
    session_id      TEXT NOT NULL REFERENCES audio_sessions(id) ON DELETE CASCADE,
    started_at_ms   INTEGER NOT NULL,
    ended_at_ms     INTEGER NOT NULL,
    segment_count   INTEGER NOT NULL,
    manifest_path   TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                      CHECK (status IN ('pending','running','processed','empty','failed')),
    attempts        INTEGER NOT NULL DEFAULT 0,
    archive_model   TEXT,
    last_error      TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at     TEXT
);
CREATE INDEX idx_audio_clips_status ON audio_clips(status);
CREATE INDEX idx_audio_clips_session ON audio_clips(session_id, started_at_ms);

-- 2) Per-segment clip linkage + clip-local speaker index for "Speaker A/B" UI.
ALTER TABLE audio_segments
    ADD COLUMN clip_id TEXT REFERENCES audio_clips(id);
ALTER TABLE audio_segments
    ADD COLUMN clip_local_speaker_idx INTEGER;
CREATE INDEX idx_segments_clip ON audio_segments(clip_id);

-- 3) Speaker kind + provisional GC timestamp. Existing rows are 'enrolled'.
ALTER TABLE speakers
    ADD COLUMN kind TEXT NOT NULL DEFAULT 'enrolled'
        CHECK (kind IN ('enrolled','provisional'));
ALTER TABLE speakers
    ADD COLUMN provisional_last_matched_at TEXT;
CREATE INDEX idx_speakers_provisional
    ON speakers(provisional_last_matched_at)
    WHERE kind = 'provisional';
