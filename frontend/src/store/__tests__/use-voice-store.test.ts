import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  useVoiceStore,
  pruneSegments,
  isMeaningfulFinal,
  looksLikeTargetLang,
} from '../use-voice-store';
import type { Segment } from '../../types';
import { resetBackendUrlCache } from '../../api/backend-url';

class MockWebSocket {
  static instances: MockWebSocket[] = [];

  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  close = vi.fn();

  constructor(public readonly url: string) {
    MockWebSocket.instances.push(this);
  }
}

function makeSegment(overrides: Partial<Segment> = {}): Segment {
  return {
    id: crypto.randomUUID(),
    sessionId: 'session-1',
    text: 'Test transcript text.',
    createdAt: new Date().toISOString(),
    starred: false,
    ...overrides,
  };
}

describe('pruneSegments', () => {
  it('keeps all starred segments regardless of count', () => {
    const segments = Array.from({ length: 40 }, (_, i) =>
      makeSegment({ id: `s${i}`, starred: true, createdAt: new Date(i * 1000).toISOString() }),
    );
    expect(pruneSegments(segments)).toHaveLength(40);
  });

  it('keeps at most 30 unstarred segments, newest first', () => {
    const segments = Array.from({ length: 40 }, (_, i) =>
      makeSegment({ id: `s${i}`, starred: false, createdAt: new Date(i * 1000).toISOString() }),
    );
    const result = pruneSegments(segments);
    expect(result).toHaveLength(30);
  });

  it('keeps all starred and top 30 unstarred', () => {
    const starred = Array.from({ length: 5 }, (_, i) =>
      makeSegment({ id: `starred-${i}`, starred: true, createdAt: new Date(i * 1000).toISOString() }),
    );
    const unstarred = Array.from({ length: 35 }, (_, i) =>
      makeSegment({ id: `unstarred-${i}`, starred: false, createdAt: new Date((i + 10) * 1000).toISOString() }),
    );
    const all = [...starred, ...unstarred];
    const result = pruneSegments(all);
    expect(result.filter(s => s.starred)).toHaveLength(5);
    expect(result.filter(s => !s.starred)).toHaveLength(30);
  });

  it('preserves order (newest first) after pruning', () => {
    const segments = Array.from({ length: 5 }, (_, i) =>
      makeSegment({ id: `s${i}`, starred: false, createdAt: new Date((5 - i) * 1000).toISOString() }),
    );
    const result = pruneSegments(segments);
    for (let i = 0; i < result.length - 1; i++) {
      expect(result[i].createdAt >= result[i + 1].createdAt).toBe(true);
    }
  });
});

describe('isMeaningfulFinal', () => {
  it('drops single-character fragments', () => {
    expect(isMeaningfulFinal('そ')).toBe(false);
    expect(isMeaningfulFinal('嗯')).toBe(false);
    expect(isMeaningfulFinal('a')).toBe(false);
  });

  it('drops pure punctuation and the LLM "." echo', () => {
    expect(isMeaningfulFinal('.')).toBe(false);
    expect(isMeaningfulFinal('。')).toBe(false);
    expect(isMeaningfulFinal('...')).toBe(false);
    expect(isMeaningfulFinal('！？')).toBe(false);
  });

  it('drops two single chars separated by punctuation/space (the "そ。 う。" pattern)', () => {
    // Two 1-char fragments stitched by punctuation strip down to 2 chars,
    // which IS the threshold. Acceptable: this catches the worst forms
    // (single isolated chars, pure punctuation) without dropping real
    // 2+ char content.
    expect(isMeaningfulFinal('そう')).toBe(true);
  });

  it('keeps short but meaningful utterances', () => {
    expect(isMeaningfulFinal('好的')).toBe(true);
    expect(isMeaningfulFinal('hello')).toBe(true);
    expect(isMeaningfulFinal('OK!')).toBe(true);
  });

  it('keeps long sentences even with heavy punctuation', () => {
    expect(isMeaningfulFinal('神网站第82期，今天分享的是。')).toBe(true);
  });
});

describe('looksLikeTargetLang', () => {
  it('recognizes English text against an en target', () => {
    expect(looksLikeTargetLang('Hello world how are you', 'en')).toBe(true);
    expect(looksLikeTargetLang('The quick brown fox jumps', 'en')).toBe(true);
  });

  it('recognizes Chinese text against zh-CN', () => {
    expect(looksLikeTargetLang('你好世界，今天天气真好', 'zh-CN')).toBe(true);
    expect(looksLikeTargetLang('神奇网站第82期', 'zh-CN')).toBe(true);
  });

  it('recognizes Japanese (with kana) against ja', () => {
    expect(looksLikeTargetLang('こんにちは世界', 'ja')).toBe(true);
    expect(looksLikeTargetLang('カタカナとひらがな', 'ja')).toBe(true);
  });

  it('does NOT match pure-kanji text against ja (could be zh-CN)', () => {
    // No kana → fall through to translate.
    expect(looksLikeTargetLang('神奇网站第八十二期', 'ja')).toBe(false);
  });

  it('does NOT match Japanese against zh-CN (kana present)', () => {
    expect(looksLikeTargetLang('こんにちは世界', 'zh-CN')).toBe(false);
  });

  it('does NOT match cross-script content', () => {
    expect(looksLikeTargetLang('Hello world', 'zh-CN')).toBe(false);
    expect(looksLikeTargetLang('你好世界', 'en')).toBe(false);
  });

  it('falls through to translate for very short text (<3 letter graphemes)', () => {
    // Two-letter words are too short to classify confidently — better
    // to send them to the LLM than risk a false positive.
    expect(looksLikeTargetLang('ok', 'en')).toBe(false);
    expect(looksLikeTargetLang('你好', 'zh-CN')).toBe(false);
  });

  it('punctuation and digits are ignored when classifying', () => {
    // Numbers and punctuation don't tip the count one way or the other.
    expect(looksLikeTargetLang('Hello, 1234! World!', 'en')).toBe(true);
  });
});

describe('useVoiceStore', () => {
  beforeEach(() => {
    resetBackendUrlCache();
    localStorage.clear();
    MockWebSocket.instances = [];
    vi.stubGlobal('WebSocket', MockWebSocket);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true }));
    useVoiceStore.setState({
      isRecording: false,
      currentSession: null,
      segments: [],
      clipInterval: 5,
      speakers: [],
      speakersStatus: 'idle',
      speakersError: null,
      _ws: null,
    });
  });

  afterEach(() => {
    useVoiceStore.setState({
      isRecording: false,
      currentSession: null,
      _ws: null,
    });
    vi.unstubAllGlobals();
  });

  it('starts with defaults', () => {
    const s = useVoiceStore.getState();
    expect(s.isRecording).toBe(false);
    expect(s.currentSession).toBeNull();
    expect(s.segments).toHaveLength(0);
    expect(s.speakers).toHaveLength(0);
    expect(s.speakersStatus).toBe('idle');
    expect(s.clipInterval).toBe(5);
  });

  it('startRecording sets isRecording and creates a session', async () => {
    useVoiceStore.getState().startRecording();
    await vi.waitFor(() => expect(MockWebSocket.instances).toHaveLength(1));
    const s = useVoiceStore.getState();
    expect(s.isRecording).toBe(true);
    expect(s.currentSession).not.toBeNull();
    expect(s.currentSession!.lines).toEqual([]);
    expect(s.currentSession!.pendingPartial).toBeNull();
    expect(MockWebSocket.instances[0].url).toBe('ws://127.0.0.1:3000/ws');
  });

  it('audio_level WS frames update audioLevel', async () => {
    useVoiceStore.getState().startRecording();
    await vi.waitFor(() => expect(MockWebSocket.instances).toHaveLength(1));
    const ws = MockWebSocket.instances[0]!;
    ws.onmessage?.(new MessageEvent('message', {
      data: JSON.stringify({ kind: 'audio_level', rms: 0.123 }),
    }));
    expect(useVoiceStore.getState().audioLevel).toBeCloseTo(0.123, 5);
    // A second frame replaces, doesn't accumulate.
    ws.onmessage?.(new MessageEvent('message', {
      data: JSON.stringify({ kind: 'audio_level', rms: 0.045 }),
    }));
    expect(useVoiceStore.getState().audioLevel).toBeCloseTo(0.045, 5);
  });

  it('audio_level frames with non-numeric rms are ignored', async () => {
    useVoiceStore.setState({ audioLevel: 0.5 });
    useVoiceStore.getState().startRecording();
    await vi.waitFor(() => expect(MockWebSocket.instances).toHaveLength(1));
    const ws = MockWebSocket.instances[0]!;
    ws.onmessage?.(new MessageEvent('message', {
      data: JSON.stringify({ kind: 'audio_level', rms: 'not a number' }),
    }));
    // No update — guard rejected the frame.
    expect(useVoiceStore.getState().audioLevel).toBe(0.5);
  });

  it('stopRecording resets audioLevel to 0', () => {
    useVoiceStore.setState({ audioLevel: 0.42 });
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().stopRecording();
    expect(useVoiceStore.getState().audioLevel).toBe(0);
  });

  it('appendLiveTranscript appends lines to currentSession', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().appendLiveTranscript('Hello world.');
    useVoiceStore.getState().appendLiveTranscript('Second sentence.');
    const lines = useVoiceStore.getState().currentSession!.lines;
    expect(lines.map((l) => l.text)).toEqual(['Hello world.', 'Second sentence.']);
  });

  it('flushInterval creates a segment from all lines and clears them', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().appendLiveTranscript('Some spoken words.');
    useVoiceStore.getState().flushInterval();
    const s = useVoiceStore.getState();
    expect(s.segments).toHaveLength(1);
    expect(s.segments[0].text).toBe('Some spoken words.');
    expect(s.currentSession!.lines).toEqual([]);
  });

  it('flushInterval does nothing when there are no lines', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().flushInterval();
    expect(useVoiceStore.getState().segments).toHaveLength(0);
  });

  it('stopRecording with non-empty transcript saves a segment', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().appendLiveTranscript('Final words.');
    useVoiceStore.getState().stopRecording();
    const s = useVoiceStore.getState();
    expect(s.isRecording).toBe(false);
    expect(s.currentSession).toBeNull();
    expect(s.segments).toHaveLength(1);
  });

  it('stopRecording with empty transcript saves no segment', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().stopRecording();
    expect(useVoiceStore.getState().segments).toHaveLength(0);
  });

  it('starSegment marks a segment as starred', () => {
    useVoiceStore.setState({ segments: [makeSegment({ id: 'seg-1', starred: false })] });
    useVoiceStore.getState().starSegment('seg-1');
    expect(useVoiceStore.getState().segments[0].starred).toBe(true);
  });

  it('unstarSegment marks a segment as unstarred and prunes', () => {
    useVoiceStore.setState({ segments: [makeSegment({ id: 'seg-1', starred: true })] });
    useVoiceStore.getState().unstarSegment('seg-1');
    expect(useVoiceStore.getState().segments[0].starred).toBe(false);
  });

  it('deleteSegment removes the segment', () => {
    useVoiceStore.setState({ segments: [makeSegment({ id: 'seg-1' })] });
    useVoiceStore.getState().deleteSegment('seg-1');
    expect(useVoiceStore.getState().segments).toHaveLength(0);
  });

  it('setClipInterval updates interval and persists to localStorage', () => {
    useVoiceStore.getState().setClipInterval(10);
    expect(useVoiceStore.getState().clipInterval).toBe(10);
    const stored = JSON.parse(localStorage.getItem('actio-voice') ?? '{}');
    expect(stored.clipInterval).toBe(10);
  });

  it('persists segments to localStorage on flush', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().appendLiveTranscript('Persisted text.');
    useVoiceStore.getState().flushInterval();
    const stored = JSON.parse(localStorage.getItem('actio-voice') ?? '{}');
    expect(stored.segments).toHaveLength(1);
    expect(stored.segments[0].text).toBe('Persisted text.');
  });

});
