// frontend/src/hooks/__tests__/useLiveSocket.test.tsx
import { renderHook, act } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useLiveSocket } from '../useLiveSocket';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';

describe('useLiveSocket', () => {
  let startSpy: ReturnType<typeof vi.fn>;
  let stopSpy: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    startSpy = vi.fn();
    stopSpy = vi.fn();
    useVoiceStore.setState({ startRecording: startSpy, stopRecording: stopSpy });
    useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: null } });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('does nothing while listeningEnabled is null (booting)', () => {
    renderHook(() => useLiveSocket());
    expect(startSpy).not.toHaveBeenCalled();
    expect(stopSpy).not.toHaveBeenCalled();
  });

  it('calls startRecording when listeningEnabled flips to true', () => {
    renderHook(() => useLiveSocket());
    act(() => {
      useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: true } });
    });
    expect(startSpy).toHaveBeenCalledTimes(1);
  });

  it('calls stopRecording when listeningEnabled flips from true to false', () => {
    useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: true } });
    renderHook(() => useLiveSocket());
    startSpy.mockClear();
    act(() => {
      useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: false } });
    });
    expect(stopSpy).toHaveBeenCalledTimes(1);
    expect(startSpy).not.toHaveBeenCalled();
  });

  it('calls startRecording on mount if already true (boot-with-on)', () => {
    useStore.setState({ ui: { ...useStore.getState().ui, listeningEnabled: true } });
    renderHook(() => useLiveSocket());
    expect(startSpy).toHaveBeenCalledTimes(1);
  });
});
