import { describe, expect, it, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { LiveTranscript } from '../LiveTranscript';
import type { TranscriptLine } from '../../store/use-voice-store';
import { useVoiceStore } from '../../store/use-voice-store';
import { LanguageProvider } from '../../i18n';

function mkLine(id: string, text: string): TranscriptLine {
  return {
    id,
    text,
    start_ms: 0,
    end_ms: 0,
    speaker_id: null,
    resolved: true,
    is_final: true,
  };
}

beforeEach(() => {
  useVoiceStore.setState({
    speakers: [],
    translation: { enabled: false, targetLang: 'en', byLineId: {} },
  });
});

describe('LiveTranscript translation rendering', () => {
  it('does not render translations when toggle is off', () => {
    useVoiceStore.setState({
      translation: {
        enabled: false,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'done', text: '你好' } },
      },
    });
    render(<LanguageProvider><LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} /></LanguageProvider>);
    expect(screen.queryByText('你好')).not.toBeInTheDocument();
    expect(screen.getByText('hello')).toBeInTheDocument();
  });

  it('renders done translation under the source line when toggle is on', () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'done', text: '你好' } },
      },
    });
    render(<LanguageProvider><LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} /></LanguageProvider>);
    expect(screen.getByText('hello')).toBeInTheDocument();
    expect(screen.getByText('你好')).toBeInTheDocument();
  });

  it('renders pending placeholder while translating', () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'pending' } },
      },
    });
    render(<LanguageProvider><LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} /></LanguageProvider>);
    expect(screen.getByText(/translating/i)).toBeInTheDocument();
  });

  it('renders error link and retry calls retryTranslationLine', () => {
    const retrySpy = vi.fn();
    useVoiceStore.setState({
      retryTranslationLine: retrySpy,
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'error' } },
      },
    });
    render(<LanguageProvider><LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} /></LanguageProvider>);
    const retry = screen.getByRole('button', { name: /retry/i });
    fireEvent.click(retry);
    expect(retrySpy).toHaveBeenCalledWith('a');
  });
});
