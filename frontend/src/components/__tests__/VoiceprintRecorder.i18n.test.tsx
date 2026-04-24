import { act, render, screen } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { VoiceprintRecorder } from '../VoiceprintRecorder';
import { LanguageProvider } from '../../i18n';

vi.mock('../../api/speakers', () => ({
  startLiveEnrollment: vi.fn().mockResolvedValue({
    status: 'active',
    captured: 0,
    target: 5,
    rms_level: 0,
  }),
  cancelLiveEnrollment: vi.fn().mockResolvedValue(undefined),
  getLiveEnrollmentStatus: vi.fn().mockResolvedValue(null),
}));

vi.mock('../../store/use-voice-store', () => ({
  useVoiceStore: (sel: (s: any) => any) => sel({ fetchSpeakers: vi.fn() }),
}));

function renderRecorder(lang: 'en' | 'zh-CN') {
  localStorage.clear();
  if (lang === 'zh-CN') localStorage.setItem('actio-language', 'zh-CN');
  return render(
    <LanguageProvider>
      <VoiceprintRecorder
        speakerId="sp-1"
        speakerName="Alice"
        onDone={vi.fn()}
        onCancel={vi.fn()}
      />
    </LanguageProvider>,
  );
}

describe('VoiceprintRecorder i18n', () => {
  beforeEach(() => {
    localStorage.clear();
    Object.defineProperty(navigator, 'language', { value: 'en-US', configurable: true });
  });

  it('renders English title and arming hint before API resolves', () => {
    renderRecorder('en');
    expect(screen.getByText('Record voiceprint for Alice')).toBeInTheDocument();
    expect(screen.getByText('Arming microphone…')).toBeInTheDocument();
  });

  it('renders Chinese title and arming hint under zh-CN locale', () => {
    renderRecorder('zh-CN');
    expect(screen.getByText('为 Alice 录制声纹')).toBeInTheDocument();
    expect(screen.getByText('正在准备麦克风…')).toBeInTheDocument();
  });

  it('shows Chinese passage and 中文 switcher chip once state is active', async () => {
    renderRecorder('zh-CN');
    await act(async () => {});
    // Passage set defaults to 'zh' when lang is zh-CN
    expect(screen.getByText('中文')).toBeInTheDocument();
    // Passage text is split across text nodes by the surrounding quotes, so
    // use a regex against the paragraph's full text content instead.
    expect(
      screen.getByText(/春天的风轻轻吹过湖面，岸边的柳树摇晃着细长的枝条/),
    ).toBeInTheDocument();
    // Meter label in Chinese (rms_level=0 → waiting, not hearing)
    expect(screen.getByText('等待声音输入…')).toBeInTheDocument();
  });

  it('shows English passage and English switcher chip by default', async () => {
    renderRecorder('en');
    await act(async () => {});
    expect(screen.getByText('English')).toBeInTheDocument();
    expect(
      screen.getByText(/The quick brown fox jumps over the lazy dog/),
    ).toBeInTheDocument();
    expect(screen.getByText('Waiting for sound…')).toBeInTheDocument();
  });
});
