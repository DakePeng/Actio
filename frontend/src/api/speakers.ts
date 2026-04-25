import type {
  EnrollResponse,
  LiveEnrollmentState,
  Speaker,
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

// ── Candidate speakers (provisional rows from batch clip processing) ──────

export interface CandidateSpeaker {
  id: string;
  display_name: string;
  color: string;
  last_matched_at: string | null;
}

export async function listCandidateSpeakers(): Promise<CandidateSpeaker[]> {
  return requestJson<CandidateSpeaker[]>('/candidate-speakers');
}

export async function promoteCandidateSpeaker(
  id: string,
  displayName?: string,
): Promise<void> {
  await requestJson<void>(`/candidate-speakers/${id}/promote`, {
    method: 'POST',
    body: JSON.stringify({ display_name: displayName ?? null }),
  });
}

export async function dismissCandidateSpeaker(id: string): Promise<void> {
  await requestJson<void>(`/candidate-speakers/${id}`, { method: 'DELETE' });
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

/** Live voiceprint enrollment: arms the backend audio pipeline to save the
 *  next `target` quality-passing VAD segments as this speaker's voiceprints. */
export async function startLiveEnrollment(
  speakerId: string,
  target = 3,
): Promise<LiveEnrollmentState> {
  return requestJson<LiveEnrollmentState>(
    `/speakers/${speakerId}/enroll-live/start`,
    {
      method: 'POST',
      body: JSON.stringify({ target }),
    },
  );
}

export async function cancelLiveEnrollment(speakerId: string): Promise<void> {
  await requestJson<void>(`/speakers/${speakerId}/enroll-live/cancel`, {
    method: 'POST',
  });
}

export async function getLiveEnrollmentStatus(): Promise<LiveEnrollmentState | null> {
  return requestJson<LiveEnrollmentState | null>('/enroll-live/status');
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
  return getApiUrl(`/candidates/audio/${encoded}`);
}
