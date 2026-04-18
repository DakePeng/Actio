import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useVoiceStore, pruneSegments } from '../use-voice-store';
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
      unknowns: [],
      dismissedUnknowns: new Set<string>(),
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
    expect(s.unknowns).toHaveLength(0);
    expect(s.speakersStatus).toBe('idle');
    expect(s.clipInterval).toBe(5);
  });

  it('startRecording sets isRecording and creates a session', async () => {
    useVoiceStore.getState().startRecording();
    await vi.waitFor(() => expect(MockWebSocket.instances).toHaveLength(1));
    const s = useVoiceStore.getState();
    expect(s.isRecording).toBe(true);
    expect(s.currentSession).not.toBeNull();
    expect(s.currentSession!.liveTranscript).toBe('');
    expect(MockWebSocket.instances[0].url).toBe('ws://127.0.0.1:3000/ws');
  });

  it('appendLiveTranscript appends text to currentSession', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().appendLiveTranscript('Hello world.');
    useVoiceStore.getState().appendLiveTranscript('Second sentence.');
    const transcript = useVoiceStore.getState().currentSession!.liveTranscript;
    expect(transcript).toContain('Hello world.');
    expect(transcript).toContain('Second sentence.');
  });

  it('flushInterval creates a segment and clears liveTranscript', () => {
    useVoiceStore.getState().startRecording();
    useVoiceStore.getState().appendLiveTranscript('Some spoken words.');
    useVoiceStore.getState().flushInterval();
    const s = useVoiceStore.getState();
    expect(s.segments).toHaveLength(1);
    expect(s.segments[0].text).toBe('Some spoken words.');
    expect(s.currentSession!.liveTranscript).toBe('');
  });

  it('flushInterval does nothing when liveTranscript is empty', () => {
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

  it('dismissUnknown soft-hides an unknown and remembers it', () => {
    useVoiceStore.setState({
      unknowns: [
        { segment_id: 'seg-a', session_id: 's', start_ms: 0, end_ms: 1000, has_embedding: true },
        { segment_id: 'seg-b', session_id: 's', start_ms: 2000, end_ms: 3000, has_embedding: true },
      ],
    });
    useVoiceStore.getState().dismissUnknown('seg-a');
    const s = useVoiceStore.getState();
    expect(s.unknowns.map((u) => u.segment_id)).toEqual(['seg-b']);
    expect(s.dismissedUnknowns.has('seg-a')).toBe(true);
  });
});
