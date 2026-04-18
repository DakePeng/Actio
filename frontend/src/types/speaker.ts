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
