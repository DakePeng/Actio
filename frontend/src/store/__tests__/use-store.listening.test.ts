import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useStore } from '../use-store';

describe('useStore — listening toggle', () => {
  beforeEach(() => {
    vi.useFakeTimers().setSystemTime(new Date('2026-04-25T12:00:00Z'));
    useStore.setState((state) => ({
      ui: {
        ...state.ui,
        listeningEnabled: null,
        listeningStartedAt: null,
      },
    }));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('setListening(true) updates the store and stamps listeningStartedAt', async () => {
    await useStore.getState().setListening(true);
    const { ui } = useStore.getState();
    expect(ui.listeningEnabled).toBe(true);
    expect(ui.listeningStartedAt).toBe(Date.parse('2026-04-25T12:00:00Z'));
  });

  it('setListening(false) clears listeningStartedAt', async () => {
    await useStore.getState().setListening(true);
    await useStore.getState().setListening(false);
    const { ui } = useStore.getState();
    expect(ui.listeningEnabled).toBe(false);
    expect(ui.listeningStartedAt).toBeNull();
  });

  it('setListening reverts state and pushes failure feedback when PATCH fails', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 500 }));
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
    }));

    await useStore.getState().setListening(false);

    const { ui } = useStore.getState();
    expect(ui.listeningEnabled).toBe(true);
    expect(ui.listeningStartedAt).toBe(1);
    expect(ui.feedback?.message).toBe('feedback.listeningToggleFailed');
    expect(fetch).toHaveBeenCalledWith(
      expect.stringContaining('/settings'),
      expect.objectContaining({ method: 'PATCH' }),
    );
  });

  it('does not revert when a newer setListening has superseded the failing one', async () => {
    // Clear any stale feedback bleeding through from a prior test's failure path
    // (module-level feedbackTimer, fake timers from beforeEach defer the auto-clear).
    useStore.setState((state) => ({ ui: { ...state.ui, feedback: null } }));

    let patchIndex = 0;
    let resolveFirst!: (v: { ok: boolean; status?: number }) => void;
    vi.stubGlobal(
      'fetch',
      vi.fn((input: unknown, init?: { method?: string }) => {
        const url = typeof input === 'string' ? input : String(input);
        // Only count PATCH /settings calls — backend-url's /health probe and
        // any other GETs should always succeed so the test isn't gated on
        // discovery state from prior tests.
        if (init?.method === 'PATCH' && url.includes('/settings')) {
          patchIndex += 1;
          if (patchIndex === 1) {
            // First PATCH — hold it open until we explicitly fail it later.
            return new Promise((resolve) => {
              resolveFirst = resolve;
            });
          }
        }
        return Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve({}) });
      }),
    );

    // Kick off the first toggle (will hang until we resolve it).
    const firstPromise = useStore.getState().setListening(false);
    // Now fire a second toggle that lands optimistically + succeeds.
    await useStore.getState().setListening(true);
    expect(useStore.getState().ui.listeningEnabled).toBe(true);

    // Fail the first PATCH after the second has already taken over.
    resolveFirst({ ok: false, status: 500 });
    await firstPromise;

    const { ui } = useStore.getState();
    expect(ui.listeningEnabled).toBe(true);
    expect(ui.feedback?.message).not.toBe('feedback.listeningToggleFailed');
  });
});
