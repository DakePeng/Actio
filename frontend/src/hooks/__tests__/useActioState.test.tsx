import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useActioState } from '../useActioState';
import { useStore } from '../../store/use-store';
import { useVoiceStore } from '../../store/use-voice-store';
import { clearWordmarkPreview } from '../useWordmarkPreview';

function Probe() {
  return <div data-testid="state">{useActioState()}</div>;
}

describe('useActioState', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    clearWordmarkPreview();
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
    act(() => {
      vi.runOnlyPendingTimers();
    });
    vi.useRealTimers();
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
