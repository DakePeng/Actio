import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useActioState } from '../useActioState';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';
import { clearWordmarkPreview } from '../useWordmarkPreview';
import { clearWordmarkFlash, flashWordmark } from '../useWordmarkFlash';

function Probe() {
  return <div data-testid="state">{useActioState()}</div>;
}

describe('useActioState', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    clearWordmarkPreview();
    clearWordmarkFlash();
    useStore.setState((state) => ({
      reminders: [],
      ui: {
        ...state.ui,
        isDictating: false,
        isDictationTranscribing: false,
        dictationTranscript: '',
        feedback: null,
      },
    }));
    useVoiceStore.setState({ isRecording: false });
  });

  afterEach(() => {
    clearWordmarkPreview();
    clearWordmarkFlash();
    act(() => {
      vi.runOnlyPendingTimers();
    });
    vi.useRealTimers();
  });

  it('maps the active dictation capture phase to "transcribing"', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, isDictating: true, isDictationTranscribing: false },
    }));

    render(<Probe />);

    expect(screen.getByTestId('state')).toHaveTextContent('transcribing');
  });

  it('maps the dictation finalize phase to "processing"', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, isDictating: false, isDictationTranscribing: true },
    }));

    render(<Probe />);

    expect(screen.getByTestId('state')).toHaveTextContent('processing');
  });

  it('shows "success" while a flash is active and reverts when it expires', () => {
    useVoiceStore.setState({ isRecording: false });

    const { rerender } = render(<Probe />);
    expect(screen.getByTestId('state')).toHaveTextContent('standby');

    act(() => {
      flashWordmark('success', 1200);
    });
    rerender(<Probe />);
    expect(screen.getByTestId('state')).toHaveTextContent('success');

    act(() => {
      vi.advanceTimersByTime(1200);
    });
    rerender(<Probe />);
    expect(screen.getByTestId('state')).toHaveTextContent('standby');
  });

  it('reverts to "listening" after a paste flash if the background pipeline is on', () => {
    useVoiceStore.setState({ isRecording: true });

    const { rerender } = render(<Probe />);

    act(() => {
      flashWordmark('success', 1200);
    });
    rerender(<Probe />);
    expect(screen.getByTestId('state')).toHaveTextContent('success');

    act(() => {
      vi.advanceTimersByTime(1200);
    });
    rerender(<Probe />);
    expect(screen.getByTestId('state')).toHaveTextContent('listening');
  });

  it('falls back to processing when reminder extraction is in flight', () => {
    useStore.setState((state) => ({
      reminders: [
        {
          id: 'extracting-1',
          title: '',
          description: '',
          priority: 'medium',
          labels: [],
          isExtracting: true,
          createdAt: new Date('2026-04-24T00:00:00.000Z').toISOString(),
          archivedAt: null,
        },
      ],
      ui: {
        ...state.ui,
        isDictating: false,
        isDictationTranscribing: false,
      },
    }));

    render(<Probe />);

    expect(screen.getByTestId('state')).toHaveTextContent('processing');
  });

});
