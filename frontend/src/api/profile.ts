import { getApiUrl } from './backend-url';
import { DEV_TENANT_ID } from './actio-api';
import { requestJson } from './http';

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
