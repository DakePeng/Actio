import { renderHook } from '@testing-library/react';
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
    // wait one microtask for listen() promise to resolve
    await Promise.resolve();
    listeners['shortcut-triggered']?.({ payload: 'toggle_listening' });
    expect(setListeningMock).toHaveBeenCalledWith(false);
  });
});
