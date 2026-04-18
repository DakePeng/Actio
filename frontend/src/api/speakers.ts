import type {
  AssignSegmentResult,
  AssignTarget,
  EnrollResponse,
  Speaker,
  UnknownSegment,
  VoiceprintCandidate,
} from '../types/speaker';
import { getApiUrl } from './backend-url';
import { DEV_TENANT_ID } from './actio-api';

async function requestJson<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(await getApiUrl(path), {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      'x-tenant-id': DEV_TENANT_ID,
      ...(init.headers ?? {}),
    },
  });
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(`Actio API ${response.status}: ${response.statusText}${text ? ` — ${text}` : ''}`);
  }
  if (response.status === 204) {
    return undefined as T;
  }
  return (await response.json()) as T;
}

export async function listSpeakers(): Promise<Speaker[]> {
  return requestJson<Speaker[]>('/speakers');
}

export async function createSpeaker(input: {
  display_name: string;
  color: string;
}): Promise<Speaker> {
  return requestJson<Speaker>('/speakers', {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export async function updateSpeaker(
  id: string,
  patch: { display_name?: string; color?: string },
): Promise<Speaker> {
  return requestJson<Speaker>(`/speakers/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(patch),
  });
}

export async function deleteSpeaker(id: string): Promise<void> {
  await requestJson<void>(`/speakers/${id}`, { method: 'DELETE' });
}

/**
 * Upload 1-N WAV clips and extract/store voiceprints. `mode=replace` deletes
 * any prior embeddings for this speaker before inserting the new ones.
 */
export async function enrollSpeaker(
  id: string,
  clips: Blob[],
): Promise<EnrollResponse> {
  const form = new FormData();
  clips.forEach((blob, i) => form.append(`clip_${i}`, blob, `clip_${i}.wav`));
  const response = await fetch(await getApiUrl(`/speakers/${id}/enroll?mode=replace`), {
    method: 'POST',
    // Let the browser set multipart Content-Type with boundary — do NOT
    // override it here.
    headers: { 'x-tenant-id': DEV_TENANT_ID },
    body: form,
  });
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(`Enroll failed (${response.status}): ${text || response.statusText}`);
  }
  return (await response.json()) as EnrollResponse;
}

export async function listUnknowns(limit = 50): Promise<UnknownSegment[]> {
  return requestJson<UnknownSegment[]>(`/unknowns?limit=${limit}`);
}

export async function listSessionUnknowns(
  sessionId: string,
  limit = 50,
): Promise<UnknownSegment[]> {
  return requestJson<UnknownSegment[]>(
    `/sessions/${sessionId}/unknowns?limit=${limit}`,
  );
}

export async function assignSegment(
  segmentId: string,
  target: AssignTarget,
): Promise<AssignSegmentResult> {
  return requestJson<AssignSegmentResult>(`/segments/${segmentId}/assign`, {
    method: 'POST',
    body: JSON.stringify(target),
  });
}

export async function unassignSegment(segmentId: string): Promise<void> {
  await requestJson<void>(`/segments/${segmentId}/unassign`, { method: 'POST' });
}

/** Phase-C: clusters of retained unknown-voice clips ready to be named. */
export async function listCandidates(): Promise<VoiceprintCandidate[]> {
  return requestJson<VoiceprintCandidate[]>('/candidates');
}

export async function confirmCandidate(input: {
  display_name: string;
  color: string;
  member_segment_ids: string[];
}): Promise<Speaker> {
  return requestJson<Speaker>('/candidates/confirm', {
    method: 'POST',
    body: JSON.stringify(input),
  });
}

export async function dismissCandidate(
  member_segment_ids: string[],
): Promise<void> {
  await requestJson<void>('/candidates/dismiss', {
    method: 'POST',
    body: JSON.stringify({ member_segment_ids }),
  });
}

/** Returns a fetchable URL for the retained clip so <audio> can play it.
 *  Resolves the backend base URL lazily so port autodiscovery still works. */
export async function candidateClipUrl(audioRef: string): Promise<string> {
  const encoded = encodeURIComponent(audioRef);
  const { getApiUrl } = await import('./backend-url');
  return getApiUrl(`/candidates/audio/${encoded}`);
}
