import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../../api/backend-url', () => ({
  getApiUrl: vi.fn(async (path: string) => `http://localhost:3000${path}`),
}));

import { LlmSettings } from '../LlmSettings';
import { LanguageProvider } from '../../../i18n';

/** Pins ISS-075: LlmSettings used to swallow `patchLlmSettings` failures on
 *  the "Local" radio onChange and `cancelAndUnselect` paths via
 *  `.catch(() => {})`. After the fix, those paths surface the failure
 *  through the same error banner that `handleSelectionChange` uses. */
describe('LlmSettings error surfacing (ISS-075)', () => {
  let fetchSpy: ReturnType<typeof vi.fn>;
  let originalFetch: typeof globalThis.fetch;

  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });

    originalFetch = globalThis.fetch;
    fetchSpy = vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
      const u = typeof url === 'string' ? url : url.toString();
      const method = init?.method ?? 'GET';

      if (u.endsWith('/settings') && method === 'GET') {
        return new Response(
          JSON.stringify({
            llm: {
              selection: { kind: 'disabled' },
              remote: {},
              local_endpoint_port: 3001,
              download_source: 'hugging_face',
              load_on_startup: false,
            },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        );
      }
      if (u.endsWith('/settings') && method === 'PATCH') {
        return new Response('boom', { status: 500 });
      }
      if (u.endsWith('/settings/llm/models')) {
        return new Response('[]', { status: 200, headers: { 'Content-Type': 'application/json' } });
      }
      if (u.endsWith('/settings/llm/load-status')) {
        return new Response(JSON.stringify({ state: 'idle' }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        });
      }
      return new Response('', { status: 404 });
    });
    globalThis.fetch = fetchSpy as unknown as typeof globalThis.fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
    vi.clearAllMocks();
  });

  it('surfaces an error banner when the Local radio fails to persist', async () => {
    render(
      <LanguageProvider>
        <LlmSettings />
      </LanguageProvider>,
    );

    // Wait for initial settings GET to settle so the radio reflects "disabled"
    // before we click "Local".
    const localRadio = await screen.findByLabelText(/Local/i);
    expect(localRadio).not.toBeChecked();

    fireEvent.click(localRadio);

    // The PATCH /settings call rejects with 500. The handleSelectionChange
    // catch path turns that into the localized "Failed to save" message.
    await waitFor(() => {
      expect(screen.getByText(/Failed to save/i)).toBeInTheDocument();
    });

    // Sanity: a PATCH request was actually attempted (i.e. the silent-catch
    // path is gone — we're routed through the error-aware handler).
    const patches = fetchSpy.mock.calls.filter(
      ([, init]) => (init as RequestInit | undefined)?.method === 'PATCH',
    );
    expect(patches.length).toBeGreaterThan(0);
  });
});
