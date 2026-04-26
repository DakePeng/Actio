import { renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Must be set before importing useGlobalShortcuts — its module-level
// `isTauri` flag is evaluated at module load time. `vi.hoisted` runs
// before any imports.
vi.hoisted(() => {
  (globalThis as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});

import { useGlobalShortcuts } from '../useGlobalShortcuts';
import { useStore } from '../../store/use-store';

const listeners: Record<string, ((e: { payload: string }) => void)> = {};

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((eventName: string, cb: (e: { payload: string }) => void) => {
    listeners[eventName] = cb;
    return Promise.resolve(() => {});
  }),
}));

describe('useGlobalShortcuts — toggle_listening', () => {
  beforeEach(() => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
    }));
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    for (const key of Object.keys(listeners)) delete listeners[key];
  });

  it('inverts the listening state when the shortcut fires', async () => {
    const setListeningMock = vi.fn().mockResolvedValue(undefined);
    useStore.setState({ setListening: setListeningMock });
    renderHook(() => useGlobalShortcuts());
    // The listen() registration is now behind a dynamic import of
    // @tauri-apps/api/event (ISSUES.md #51), so wait until the mock has
    // captured the handler before firing.
    await waitFor(() => expect(listeners['shortcut-triggered']).toBeDefined());
    listeners['shortcut-triggered']?.({ payload: 'toggle_listening' });
    await waitFor(() => expect(setListeningMock).toHaveBeenCalledWith(false));
  });

  it('toggles from false to true when fired while muted', async () => {
    const setListeningMock = vi.fn().mockResolvedValue(undefined);
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
      setListening: setListeningMock,
    }));
    renderHook(() => useGlobalShortcuts());
    await Promise.resolve();
    listeners['shortcut-triggered']?.({ payload: 'toggle_listening' });
    await Promise.resolve();
    expect(setListeningMock).toHaveBeenCalledWith(true);
  });

  it('does not call setListening when listening state is unknown (null)', async () => {
    const setListeningMock = vi.fn().mockResolvedValue(undefined);
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: null, listeningStartedAt: null },
      setListening: setListeningMock,
    }));
    renderHook(() => useGlobalShortcuts());
    await Promise.resolve();
    listeners['shortcut-triggered']?.({ payload: 'toggle_listening' });
    await Promise.resolve();
    expect(setListeningMock).not.toHaveBeenCalled();
  });

  it('emits the success toast only after setListening resolves without rollback', async () => {
    let resolveSetListening!: () => void;
    const setListeningMock = vi.fn(() =>
      new Promise<void>((resolve) => {
        resolveSetListening = resolve;
      }),
    );
    const setFeedbackMock = vi.fn();
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: 1 },
      setListening: setListeningMock,
      setFeedback: setFeedbackMock,
    }));

    renderHook(() => useGlobalShortcuts());
    await Promise.resolve();
    listeners['shortcut-triggered']?.({ payload: 'toggle_listening' });

    // Before setListening resolves, no toast yet.
    expect(setFeedbackMock).not.toHaveBeenCalled();

    // Simulate the PATCH succeeding: store state already optimistically reflects
    // the intended new value (false). Resolve the in-flight setListening.
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false },
    }));
    resolveSetListening();
    await Promise.resolve();
    await Promise.resolve();

    expect(setFeedbackMock).toHaveBeenCalledWith('feedback.listeningOff', 'success');
  });
});
