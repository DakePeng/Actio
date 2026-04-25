import { render, screen } from '@testing-library/react';
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

describe('LiveTab', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: true, listeningStartedAt: Date.now() },
    }));
    useVoiceStore.setState({ currentSession: null, isRecording: false });
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve({}) }));
  });

  it('shows the on-state header label when listening', () => {
    renderTab();
    expect(screen.getByText('Listening')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /click to mute/i })).toBeInTheDocument();
  });

  it('shows the muted hint when listening is off', () => {
    useStore.setState((state) => ({
      ui: { ...state.ui, listeningEnabled: false, listeningStartedAt: null },
    }));
    renderTab();
    expect(screen.getByText('Muted')).toBeInTheDocument();
    expect(screen.getByText(/Listening is paused/i)).toBeInTheDocument();
  });

  it('does not render manual record button', () => {
    renderTab();
    expect(screen.queryByRole('button', { name: /start.*transcribing/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /stop.*transcribing/i })).not.toBeInTheDocument();
  });
});
