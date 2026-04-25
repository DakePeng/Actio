import { render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { StandbyTray } from '../StandbyTray';
import { LanguageProvider } from '../../i18n';
import { useStore } from '../../store/use-store';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(),
}));

vi.mock('../../hooks/useActioState', () => ({
  useActioState: () => 'processing',
}));

describe('StandbyTray', () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
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

  it('shows live dictation text beside a compact tray mark', () => {
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
    expect(screen.getByLabelText('Actio')).toHaveAttribute('viewBox', '0 16 50 48');
  });
});
