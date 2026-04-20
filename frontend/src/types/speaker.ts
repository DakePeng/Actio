export interface Speaker {
  id: string;
  tenant_id: string;
  display_name: string;
  color: string;
  status: 'active' | 'inactive';
  created_at: string;
}

export interface EnrolledEmbedding {
  id: string;
  duration_ms: number;
  quality_score: number;
  is_primary: boolean;
}

export interface EnrollResponse {
  speaker_id: string;
  embeddings: EnrolledEmbedding[];
  warnings: string[];
}

/** Live-enrollment state: backend routes quality-passing VAD segments into
 *  the target speaker's voiceprints instead of the normal identify path. */
export type LiveEnrollmentStatus = 'active' | 'complete' | 'cancelled';

export interface LiveEnrollmentState {
  speaker_id: string;
  target: number;
  captured: number;
  last_captured_duration_ms?: number;
  status: LiveEnrollmentStatus;
  version: number;
  /** Smoothed RMS of recent mic audio (roughly 0..0.3 for normal speech).
   *  Drives the live mic-level meter in the enrollment UI. */
  rms_level: number;
  /** `too_short` / `too_long` / `low_quality` — set when a VAD segment
   *  failed the quality gates; cleared on the next successful capture. */
  last_rejected_reason?: string;
}

/** Phase-C voiceprint-candidate: one cluster of retained unknown-voice clips
 *  that has cleared the evidence bar (≥5 occurrences, ≥60 s cumulative,
 *  ≥2 distinct sessions). Drives the Pending Voices panel in PeopleTab. */
export interface VoiceprintCandidate {
  candidate_id: string;
  representative_segment_id: string;
  audio_ref: string;
  session_id: string;
  occurrences: number;
  total_duration_ms: number;
  earliest_ms: number;
  latest_ms: number;
  member_segment_ids: string[];
}
