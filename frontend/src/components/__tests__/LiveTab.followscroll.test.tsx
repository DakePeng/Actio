import { act, fireEvent, render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LiveTab } from '../LiveTab';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';
import type { TranscriptLine } from '../../store/use-voice-store';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

function makeLine(id: string, text: string): TranscriptLine {
  return {
    id,
    text,
    start_ms: 0,
    end_ms: 1_000,
    speaker_id: null,
    resolved: false,
    is_final: true,
  };
}

function renderTab() {
  return render(
    <LanguageProvider>
      <LiveTab />
    </LanguageProvider>,
  );
}

describe('LiveTab — follow-live scroll (ISSUES.md #57)', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: Date.now() },
    }));
    useVoiceStore.setState({
      currentSession: {
        id: 's1',
        startedAt: new Date().toISOString(),
        lines: [makeLine('a', 'hello')],
        pendingPartial: null,
        pipelineReady: true,
      },
      isRecording: true,
    });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  it('auto-scrolls to bottom when the user is at (or near) the bottom', async () => {
    const { container } = renderTab();
    const main = container.querySelector('main.live-tab__main') as HTMLElement;
    expect(main).toBeTruthy();

    // Spy on scrollTop assignment.
    const setScrollTop = vi.fn();
    Object.defineProperty(main, 'scrollHeight', { configurable: true, get: () => 1_000 });
    Object.defineProperty(main, 'clientHeight', { configurable: true, get: () => 500 });
    Object.defineProperty(main, 'scrollTop', {
      configurable: true,
      get: () => 480, // distance from bottom: 1000 - 480 - 500 = 20 px → at-bottom
      set: setScrollTop,
    });
    // Fire a scroll event so the at-bottom flag is captured pre-content.
    fireEvent.scroll(main);

    // Add a new line — effect should re-fire and scrollTop should be assigned.
    act(() => {
      useVoiceStore.setState((s) => ({
        currentSession: s.currentSession
          ? { ...s.currentSession, lines: [...s.currentSession.lines, makeLine('b', 'world')] }
          : s.currentSession,
      }));
    });

    expect(setScrollTop).toHaveBeenCalled();
    // The latest assigned value should be scrollHeight (= 1000).
    expect(setScrollTop.mock.calls[setScrollTop.mock.calls.length - 1]![0]).toBe(1_000);
  });

  it('does NOT auto-scroll when the user has scrolled up to read', async () => {
    const { container } = renderTab();
    const main = container.querySelector('main.live-tab__main') as HTMLElement;

    const setScrollTop = vi.fn();
    Object.defineProperty(main, 'scrollHeight', { configurable: true, get: () => 1_000 });
    Object.defineProperty(main, 'clientHeight', { configurable: true, get: () => 500 });
    Object.defineProperty(main, 'scrollTop', {
      configurable: true,
      get: () => 100, // distance from bottom: 1000 - 100 - 500 = 400 px → reading
      set: setScrollTop,
    });
    fireEvent.scroll(main);

    // Reset the spy: the initial first-render effect happens before the
    // scroll handler updates the ref, so we may capture a single seed
    // assignment. The behaviour we care about is the post-read content arrival.
    setScrollTop.mockClear();

    act(() => {
      useVoiceStore.setState((s) => ({
        currentSession: s.currentSession
          ? {
              ...s.currentSession,
              pendingPartial: makeLine('p', 'incoming partial...'),
            }
          : s.currentSession,
      }));
    });

    // No yank.
    expect(setScrollTop).not.toHaveBeenCalled();
  });

  it('resumes auto-scroll when the user scrolls back near the bottom', async () => {
    const { container } = renderTab();
    const main = container.querySelector('main.live-tab__main') as HTMLElement;

    let currentScrollTop = 100;
    const setScrollTop = vi.fn();
    Object.defineProperty(main, 'scrollHeight', { configurable: true, get: () => 1_000 });
    Object.defineProperty(main, 'clientHeight', { configurable: true, get: () => 500 });
    Object.defineProperty(main, 'scrollTop', {
      configurable: true,
      get: () => currentScrollTop,
      set: setScrollTop,
    });

    // First: user is reading (far from bottom). New content should not yank.
    fireEvent.scroll(main);
    setScrollTop.mockClear();
    act(() => {
      useVoiceStore.setState((s) => ({
        currentSession: s.currentSession
          ? { ...s.currentSession, lines: [...s.currentSession.lines, makeLine('c', 'mid-read')] }
          : s.currentSession,
      }));
    });
    expect(setScrollTop).not.toHaveBeenCalled();

    // Now: user scrolls back near the bottom and a new line arrives.
    currentScrollTop = 480; // within 64 px of bottom
    fireEvent.scroll(main);
    setScrollTop.mockClear();
    act(() => {
      useVoiceStore.setState((s) => ({
        currentSession: s.currentSession
          ? { ...s.currentSession, lines: [...s.currentSession.lines, makeLine('d', 'rejoined')] }
          : s.currentSession,
      }));
    });
    expect(setScrollTop).toHaveBeenCalled();
  });
});
