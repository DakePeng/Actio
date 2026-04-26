import { getApiUrl } from './backend-url';
import { DEV_TENANT_ID } from './actio-api';

export async function requestJson<T>(path: string, init: RequestInit = {}): Promise<T> {
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
