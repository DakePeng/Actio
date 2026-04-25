const API_BASE = 'http://127.0.0.1:3000';

export async function patchSettings(patch: Record<string, unknown>): Promise<void> {
  const res = await fetch(`${API_BASE}/settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  });
  if (!res.ok) throw new Error(`PATCH /settings failed: ${res.status}`);
}

export async function fetchSettings(): Promise<{ audio?: { always_listening?: boolean } }> {
  const res = await fetch(`${API_BASE}/settings`);
  if (!res.ok) throw new Error(`GET /settings failed: ${res.status}`);
  return res.json();
}
