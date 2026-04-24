import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';

vi.mock('../ProfileSection', () => ({ ProfileSection: () => <div>stub-profile</div> }));
vi.mock('../PreferencesSection', () => ({ PreferencesSection: () => <div>stub-preferences</div> }));
vi.mock('../TraySection', () => ({ TraySection: () => <div>stub-tray</div> }));
vi.mock('../LabelManager', () => ({ LabelManager: () => <div>stub-labels</div> }));
vi.mock('../AudioSettings', () => ({ AudioSettings: () => <div>stub-audio</div> }));
vi.mock('../RecordingSection', () => ({ RecordingSection: () => <div>stub-recording</div> }));
vi.mock('../LlmSettings', () => ({ LlmSettings: () => <div>stub-llm</div> }));
vi.mock('../ModelSetup', () => ({ ModelSetup: () => <div>stub-model-setup</div> }));
vi.mock('../KeyboardSettings', () => ({ KeyboardSettings: () => <div>stub-keyboard</div> }));

import { SettingsView } from '../SettingsView';
import { LanguageProvider } from '../../../i18n';

// Force English so the hardcoded label assertions in this test still hold.
beforeEach(() => {
  localStorage.clear();
  Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
});

function renderView() {
  return render(
    <LanguageProvider>
      <SettingsView />
    </LanguageProvider>,
  );
}

describe('SettingsView', () => {
  it('defaults to the General tab and renders its subsections', () => {
    renderView();
    expect(screen.getByRole('tab', { name: 'General' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByText('stub-profile')).toBeInTheDocument();
    expect(screen.getByText('stub-preferences')).toBeInTheDocument();
    expect(screen.getByText('stub-tray')).toBeInTheDocument();
    expect(screen.queryByText('stub-llm')).not.toBeInTheDocument();
  });

  it('switches panels when a different tab is clicked', async () => {
    renderView();
    await act(async () => {
      fireEvent.click(screen.getByRole('tab', { name: 'AI' }));
    });
    expect(screen.getByRole('tab', { name: 'AI' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tab', { name: 'General' })).toHaveAttribute('aria-selected', 'false');
    expect(await screen.findByText('stub-llm')).toBeInTheDocument();
    expect(screen.queryByText('stub-model-setup')).not.toBeInTheDocument();
    expect(screen.queryByText('stub-profile')).not.toBeInTheDocument();
  });

  it('renders all five tabs in order', () => {
    renderView();
    const labels = screen.getAllByRole('tab').map((el) => el.textContent);
    expect(labels).toEqual(['General', 'Board', 'Voice', 'AI', 'Shortcuts']);
  });

  it('renders tab labels in Chinese when locale is zh-CN', () => {
    localStorage.setItem('actio-language', 'zh-CN');
    renderView();
    const labels = screen.getAllByRole('tab').map((el) => el.textContent);
    expect(labels).toEqual(['常规', '看板', '语音', 'AI', '快捷键']);
  });
});
