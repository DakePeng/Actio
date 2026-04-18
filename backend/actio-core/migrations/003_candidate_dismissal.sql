-- Migration 003: record user dismissals of voiceprint-candidate clusters
-- so we don't re-surface them in `GET /candidates` after the user says
-- "not a voice". NULL means undecided (candidate still eligible).

ALTER TABLE audio_segments
    ADD COLUMN dismissed_at TEXT;
