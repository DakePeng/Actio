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
  });
});
