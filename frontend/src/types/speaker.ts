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

export interface UnknownSegment {
  segment_id: string;
  session_id: string;
  start_ms: number;
  end_ms: number;
}

export type AssignTarget =
  | { speaker_id: string }
  | { new_speaker: { display_name: string; color: string } };

export interface AssignSegmentResult {
  segment_id: string;
  speaker_id: string;
}

/** Phase-C voiceprint-candidate: one cluster of retained unknown-voice clips
 *  that has cleared the evidence bar (≥5 occurrences, ≥60 s cumulative,
 *  ≥2 distinct sessions). Drives the "who was this?" prompt modal. */
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
