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
    translation: { enabled: false, targetLang: 'en', byLineId: {}, cache: {} },
  });
});

describe('LiveTranscript translation rendering', () => {
  it('does not render translations when toggle is off', () => {
    useVoiceStore.setState({
      translation: {
        enabled: false,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'done', text: '你好' } },
        cache: {},
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
        cache: {},
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
        cache: {},
      },
    });
    render(<LanguageProvider><LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} /></LanguageProvider>);
    expect(screen.getByText(/translating/i)).toBeInTheDocument();
  });

  it('collapses consecutive pending lines into one translating placeholder', () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'pending' },
          b: { status: 'pending' },
          c: { status: 'pending' },
          d: { status: 'pending' },
        },
        cache: {},
      },
    });
    render(
      <LanguageProvider>
        <LiveTranscript
          lines={[
            mkLine('a', '有问嗯'),
            mkLine('b', '一二三'),
            mkLine('c', '资源的分布'),
            mkLine('d', '做一个好的基础设计'),
          ]}
          pendingPartial={null}
        />
      </LanguageProvider>,
    );
    // 4 pending lines should render exactly ONE translating indicator.
    const indicators = screen.getAllByText(/translating/i);
    expect(indicators).toHaveLength(1);
  });

  it('collapses consecutive error lines and retry hits all of them', () => {
    const retrySpy = vi.fn();
    useVoiceStore.setState({
      retryTranslationLine: retrySpy,
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'error' },
          b: { status: 'error' },
        },
        cache: {},
      },
    });
    render(
      <LanguageProvider>
        <LiveTranscript
          lines={[mkLine('a', '一二三'), mkLine('b', '四五六')]}
          pendingPartial={null}
        />
      </LanguageProvider>,
    );
    const retries = screen.getAllByRole('button', { name: /retry/i });
    expect(retries).toHaveLength(1);
    fireEvent.click(retries[0]!);
    expect(retrySpy).toHaveBeenCalledTimes(2);
    expect(retrySpy).toHaveBeenCalledWith('a');
    expect(retrySpy).toHaveBeenCalledWith('b');
  });

  it('splits chunks across status boundaries (done in the middle)', () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'pending' },
          b: { status: 'done', text: 'second' },
          c: { status: 'pending' },
        },
        cache: {},
      },
    });
    render(
      <LanguageProvider>
        <LiveTranscript
          lines={[
            mkLine('a', '第一'),
            mkLine('b', '第二'),
            mkLine('c', '第三'),
          ]}
          pendingPartial={null}
        />
      </LanguageProvider>,
    );
    // Two distinct pending chunks (a) and (c), separated by the done b.
    expect(screen.getAllByText(/translating/i)).toHaveLength(2);
    expect(screen.getByText('second')).toBeInTheDocument();
  });

  it('suppresses translation when output equals source (already-in-target passthrough)', () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'done', text: '你好' } },
        cache: {},
      },
    });
    render(<LanguageProvider><LiveTranscript lines={[mkLine('a', '你好')]} pendingPartial={null} /></LanguageProvider>);
    // Source visible exactly once, no duplicate translation annotation.
    expect(screen.getAllByText('你好')).toHaveLength(1);
  });

  it('renders error link and retry calls retryTranslationLine', () => {
    const retrySpy = vi.fn();
    useVoiceStore.setState({
      retryTranslationLine: retrySpy,
      translation: {
        enabled: true,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'error' } },
        cache: {},
      },
    });
    render(<LanguageProvider><LiveTranscript lines={[mkLine('a', 'hello')]} pendingPartial={null} /></LanguageProvider>);
    const retry = screen.getByRole('button', { name: /retry/i });
    fireEvent.click(retry);
    expect(retrySpy).toHaveBeenCalledWith('a');
  });
});
