import { describe, expect, it, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
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

beforeEach(() => {
  Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
  useStore.setState({
    ui: { ...useStore.getState().ui, listeningEnabled: true, listeningStartedAt: Date.now() },
  });
  useVoiceStore.setState({
    isRecording: true,
    currentSession: {
      id: 'live',
      startedAt: '',
      lines: [],
      pendingPartial: null,
      pipelineReady: true,
    },
    translation: { enabled: false, targetLang: 'en', byLineId: {}, cache: {} },
  });
});

describe('LiveTab translation controls', () => {
  it('renders the toggle pill and target select', () => {
    renderTab();
    expect(screen.getByRole('button', { name: /translate/i })).toBeInTheDocument();
    expect(screen.getByRole('combobox', { name: /target language/i })).toBeInTheDocument();
  });

  it('clicking the toggle calls setTranslationEnabled', () => {
    const spy = vi.fn().mockResolvedValue(undefined);
    useVoiceStore.setState({ setTranslationEnabled: spy });
    renderTab();
    fireEvent.click(screen.getByRole('button', { name: /translate/i }));
    expect(spy).toHaveBeenCalledWith(true);
  });

  it('changing the select calls setTranslationTargetLang', () => {
    const spy = vi.fn().mockResolvedValue(undefined);
    useVoiceStore.setState({ setTranslationTargetLang: spy });
    renderTab();
    const select = screen.getByRole('combobox', { name: /target language/i });
    fireEvent.change(select, { target: { value: 'zh-CN' } });
    expect(spy).toHaveBeenCalledWith('zh-CN');
  });

  it('select is disabled while toggle is off', () => {
    renderTab();
    const select = screen.getByRole('combobox', { name: /target language/i });
    expect(select).toBeDisabled();
  });

  it('select is enabled while toggle is on', () => {
    useVoiceStore.setState({
      translation: { enabled: true, targetLang: 'en', byLineId: {}, cache: {} },
    });
    renderTab();
    const select = screen.getByRole('combobox', { name: /target language/i });
    expect(select).toBeEnabled();
  });
});
