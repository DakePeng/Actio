import { render } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
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

describe('VoiceWave', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: Date.now() },
    }));
    useVoiceStore.setState({
      currentSession: {
        id: 'live',
        startedAt: '',
        lines: [],
        pendingPartial: null,
        pipelineReady: true,
      },
      audioLevel: 0,
    });
  });

  it('renders --idle when not listening', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
    }));
    const { container } = renderTab();
    const wave = container.querySelector('.voice-wave');
    expect(wave).not.toBeNull();
    expect(wave!.classList.contains('voice-wave--idle')).toBe(true);
    expect(wave!.classList.contains('voice-wave--live')).toBe(false);
  });

  it('renders --live when listening', () => {
    const { container } = renderTab();
    const wave = container.querySelector('.voice-wave');
    expect(wave).not.toBeNull();
    expect(wave!.classList.contains('voice-wave--live')).toBe(true);
  });

  it('renders 5 bars with --bar-h heights driven by audioLevel', () => {
    useVoiceStore.setState({ audioLevel: 0.1 });
    const { container } = renderTab();
    const bars = container.querySelectorAll<HTMLElement>('.voice-wave__bar');
    expect(bars).toHaveLength(5);
    // gain = min(1, 0.1 * 6) = 0.6; bars range from base 4 + 0.6 * amp * 26.
    // Middle bar (i=2, amp 1.0) should be the tallest.
    const heights = Array.from(bars).map((b) => b.style.getPropertyValue('--bar-h'));
    expect(heights[0]).not.toBe('');
    // Bar 2 (middle, amp 1.0) is tallest; bar 0 (amp 0.55) is shortest.
    const parsePx = (s: string) => parseFloat(s.replace('px', ''));
    expect(parsePx(heights[2]!)).toBeGreaterThan(parsePx(heights[0]!));
    expect(parsePx(heights[2]!)).toBeGreaterThan(parsePx(heights[4]!));
  });

  it('idle bars get 4px regardless of audioLevel', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
    }));
    useVoiceStore.setState({ audioLevel: 0.5 });
    const { container } = renderTab();
    const bars = container.querySelectorAll<HTMLElement>('.voice-wave__bar');
    // All bars compute to 4 + 0 * amp * 26 = 4px when not listening.
    for (const bar of Array.from(bars)) {
      expect(bar.style.getPropertyValue('--bar-h')).toBe('4px');
    }
  });
});
