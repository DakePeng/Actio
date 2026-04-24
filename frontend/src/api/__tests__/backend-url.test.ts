import { beforeEach, describe, expect, test, vi } from 'vitest';
import { getApiBaseUrl, getWsUrl, resetBackendUrlCache } from '../backend-url';

describe('backend URL discovery', () => {
  beforeEach(() => {
    resetBackendUrlCache();
    vi.restoreAllMocks();
  });

  test('uses the first healthy fallback port', async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new Error('port 3000 unavailable'))
      .mockResolvedValueOnce({ ok: true });
    vi.stubGlobal('fetch', fetchMock);

    await expect(getApiBaseUrl()).resolves.toBe('http://127.0.0.1:3001');
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:3000/health', {
      method: 'GET',
      signal: expect.any(AbortSignal),
    });
    expect(fetchMock).toHaveBeenCalledWith('http://127.0.0.1:3001/health', {
      method: 'GET',
      signal: expect.any(AbortSignal),
    });
  });

  test('builds websocket URLs from the discovered backend port', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true }));

    await expect(getWsUrl('/ws')).resolves.toBe('ws://127.0.0.1:3000/ws');
  });
});
