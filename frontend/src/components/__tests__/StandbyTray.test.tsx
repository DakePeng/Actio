import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { StandbyTray } from '../StandbyTray';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';
import { useActioState } from '../../hooks/useActioState';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(),
}));

vi.mock('../../hooks/useActioState');
const mockedUseActioState = vi.mocked(useActioState);

describe('StandbyTray', () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    mockedUseActioState.mockReturnValue('processing');
    useStore.setState((state) => ({
      reminders: [],
      ui: {
        ...state.ui,
        showBoardWindow: false,
        trayExpanded: false,
        isDictating: false,
        isDictationTranscribing: false,
        dictationTranscript: '',
        feedback: null,
      },
    }));
  });

  it('renders only the wordmark in the tray trigger', () => {
    render(
      <LanguageProvider>
        <StandbyTray />
      </LanguageProvider>,
    );

    expect(screen.queryByText('processing, reducing')).not.toBeInTheDocument();
    expect(screen.getByLabelText('Actio')).toBeInTheDocument();
  });

  it('switches to the compact tray glyph and shows the live transcript while transcribing', () => {
    mockedUseActioState.mockReturnValue('transcribing');
    useStore.setState((state) => ({
      ui: {
        ...state.ui,
        isDictating: true,
        dictationTranscript: 'Schedule the design review for tomorrow morning.',
      },
    }));

    render(
      <LanguageProvider>
        <StandbyTray />
      </LanguageProvider>,
    );

    expect(screen.getByText('Listening...')).toBeInTheDocument();
    expect(screen.getByText('Schedule the design review for tomorrow morning.')).toBeInTheDocument();
    // Compact tray view — viewBox crops to the primary 'a' glyph at the
    // same physical size it has inside the full wordmark.
    expect(screen.getByLabelText('Actio')).toHaveAttribute('viewBox', '0 16 44 48');
  });
});
