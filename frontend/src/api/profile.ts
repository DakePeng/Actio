import { getApiUrl } from './backend-url';
import { DEV_TENANT_ID } from './actio-api';

export type ProfileResponse = {
  tenant_id: string;
  display_name: string | null;
  aliases: string[];
  bio: string | null;
};

export type UpdateProfileRequest = {
  display_name?: string | null;
  aliases?: string[];
  bio?: string | null;
};

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

export async function fetchProfile(): Promise<ProfileResponse | null> {
  const response = await fetch(await getApiUrl('/profile'), {
    headers: { 'x-tenant-id': DEV_TENANT_ID },
  });
  if (response.status === 404) return null;
  if (!response.ok) {
    const text = await response.text().catch(() => '');
    throw new Error(`fetchProfile failed (${response.status}): ${text || response.statusText}`);
  }
  return (await response.json()) as ProfileResponse;
}

export async function updateProfile(req: UpdateProfileRequest): Promise<ProfileResponse> {
  return requestJson<ProfileResponse>('/profile', {
    method: 'PUT',
    body: JSON.stringify(req),
  });
}
