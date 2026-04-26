import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CandidateSpeakersPanel } from '../CandidateSpeakersPanel';
import { LanguageProvider } from '../../i18n';
import { resetBackendUrlCache } from '../../api/backend-url';

function ok(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

function makeFetchMock(handlers: Record<string, () => Response | Promise<Response>>) {
  return vi.fn(async (url: RequestInfo | URL) => {
    const path = typeof url === 'string' ? url : url.toString();
    // The backend-url discovery probes /health first. Always resolve it
    // OK so getApiBaseUrl picks the first candidate port.
    if (path.endsWith('/health')) {
      return ok({});
    }
    // Match longest pattern first so '/candidate-speakers/:id/promote'
    // wins over '/candidate-speakers'.
    const sorted = Object.entries(handlers).sort(
      (a, b) => b[0].length - a[0].length,
    );
    for (const [pattern, handler] of sorted) {
      if (path.includes(pattern)) {
        return handler();
      }
    }
    throw new Error(`unexpected fetch: ${path}`);
  });
}

function renderPanel() {
  return render(
    <LanguageProvider>
      <CandidateSpeakersPanel />
    </LanguageProvider>,
  );
}

describe('CandidateSpeakersPanel', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', {
      value: 'en-US',
      configurable: true,
    });
    resetBackendUrlCache();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    resetBackendUrlCache();
  });

  it('renders the empty state when no candidates exist', async () => {
    vi.stubGlobal(
      'fetch',
      makeFetchMock({
        '/candidate-speakers': () =>
          new Response('[]', {
            status: 200,
            headers: { 'Content-Type': 'application/json' },
          }),
      }),
    );
    renderPanel();
    await waitFor(() => {
      expect(screen.getByText(/no suggestions right now/i)).toBeInTheDocument();
    });
    expect(screen.getByText(/suggested people/i)).toBeInTheDocument();
  });

  it('lists candidates with display name + relative timestamp', async () => {
    const recent = new Date(Date.now() - 5 * 60_000).toISOString();
    vi.stubGlobal(
      'fetch',
      makeFetchMock({
        '/candidate-speakers': () =>
          new Response(
            JSON.stringify([
              {
                id: '11111111-1111-1111-1111-111111111111',
                display_name: 'Unknown 2026-04-25 14:30',
                color: '#9E9E9E',
                last_matched_at: recent,
              },
            ]),
            { status: 200, headers: { 'Content-Type': 'application/json' } },
          ),
      }),
    );
    renderPanel();
    expect(
      await screen.findByText('Unknown 2026-04-25 14:30'),
    ).toBeInTheDocument();
    // Relative timestamp formatter: 5 min back → "5m ago"
    expect(screen.getByText(/5m ago/)).toBeInTheDocument();
  });

  it('promote button reveals an inline name input', async () => {
    vi.stubGlobal(
      'fetch',
      makeFetchMock({
        '/candidate-speakers': () =>
          new Response(
            JSON.stringify([
              {
                id: '22222222-2222-2222-2222-222222222222',
                display_name: 'Unknown',
                color: '#9E9E9E',
                last_matched_at: null,
              },
            ]),
            { status: 200, headers: { 'Content-Type': 'application/json' } },
          ),
      }),
    );
    renderPanel();
    const promoteBtn = await screen.findByRole('button', { name: /^promote/i });
    fireEvent.click(promoteBtn);
    const input = await screen.findByPlaceholderText(/their name/i);
    expect(input).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /save/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /cancel/i })).toBeInTheDocument();
  });

  it('cancel from edit mode returns to the promote/dismiss view', async () => {
    vi.stubGlobal(
      'fetch',
      makeFetchMock({
        '/candidate-speakers': () =>
          new Response(
            JSON.stringify([
              {
                id: '33333333-3333-3333-3333-333333333333',
                display_name: 'Unknown',
                color: '#9E9E9E',
                last_matched_at: null,
              },
            ]),
            { status: 200, headers: { 'Content-Type': 'application/json' } },
          ),
      }),
    );
    renderPanel();
    const promoteBtn = await screen.findByRole('button', { name: /^promote/i });
    fireEvent.click(promoteBtn);
    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));
    expect(
      await screen.findByRole('button', { name: /^promote/i }),
    ).toBeInTheDocument();
    expect(screen.queryByPlaceholderText(/their name/i)).toBeNull();
  });
});
