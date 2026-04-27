import { act, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LiveTab } from '../LiveTab';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

function renderTab() {
  return render(
    <LanguageProvider>
      <LiveTab />
    </LanguageProvider>,
  );
}

/** Pins ISS-079: the duration pill no longer carries `aria-live`, and the
 *  one-shot status region only updates on listening on/off transitions —
 *  not on the per-second `now` ticks that drive the visible timer. */
describe('LiveTab aria-live behaviour (ISS-079)', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    useVoiceStore.setState({ currentSession: null, isRecording: false });
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
    }));
    vi.useFakeTimers();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('the visible "Listening since" pill no longer carries aria-live', () => {
    useStore.setState((state) => ({
      ui: {
        ...state.ui,
        listeningEnabled: true,
        listeningStartedAt: Date.parse('2026-04-27T09:42:00Z'),
      },
    }));
    const { container } = renderTab();
    const pill = container.querySelector('.live-tab__since');
    expect(pill).toBeTruthy();
    expect(pill?.getAttribute('aria-live')).toBeNull();
  });

  it('initial mount with listening off does NOT announce a stopped message', () => {
    const { container } = renderTab();
    const status = container.querySelector('[role="status"]') as HTMLElement | null;
    expect(status).toBeTruthy();
    // No transition has happened yet; assistive tech sees an empty status.
    expect(status!.textContent ?? '').toBe('');
  });

  it('flipping to listening:on announces the started-at message exactly once', async () => {
    const { container } = renderTab();
    const status = container.querySelector('[role="status"]') as HTMLElement;

    // Initial: empty
    expect(status.textContent).toBe('');

    // Flip to on with a stable started-at timestamp.
    const startedAt = Date.parse('2026-04-27T09:42:00Z');
    await act(async () => {
      useStore.setState((state) => ({
        ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: startedAt },
      }));
    });

    const firstAnnouncement = status.textContent ?? '';
    expect(firstAnnouncement).toMatch(/Listening started at/);

    // Advance the duration timer (simulating multiple per-second ticks).
    // The status text MUST stay identical so screen readers don't re-announce.
    await act(async () => {
      vi.advanceTimersByTime(5_000);
    });
    expect(status.textContent ?? '').toBe(firstAnnouncement);
  });

  it('flipping back to listening:off swaps the message to the stopped string', async () => {
    const { container } = renderTab();
    const status = container.querySelector('[role="status"]') as HTMLElement;

    // On then off.
    await act(async () => {
      useStore.setState((state) => ({
        ui: {
          ...state.ui,
          listeningEnabled: true,
          listeningStartedAt: Date.parse('2026-04-27T09:42:00Z'),
        },
      }));
    });
    expect(status.textContent ?? '').toMatch(/Listening started at/);

    await act(async () => {
      useStore.setState((state) => ({
        ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
      }));
    });
    expect(status.textContent ?? '').toMatch(/Listening stopped/);
  });
});
