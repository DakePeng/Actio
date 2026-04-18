import { create } from 'zustand';
import type { Segment } from '../types';
import type { Speaker } from '../types/speaker';
import { getWsUrl } from '../api/backend-url';
import * as speakerApi from '../api/speakers';

export type ClipInterval = 1 | 2 | 5 | 10 | 30;

/** A single finalized transcript line. `speaker_id` starts null and fills
 *  in when the backend emits a `speaker_resolved` WS event whose time
 *  range overlaps this line's [start_ms, end_ms]. */
export interface TranscriptLine {
  id: string;
  text: string;
  start_ms: number;
  end_ms: number;
  speaker_id: string | null;
  is_final: boolean;
}

interface RecordingSession {
  id: string;
  startedAt: string;
  /** Finalized ASR output, per line. Rendered as speaker-grouped bubbles. */
  lines: TranscriptLine[];
  /** In-progress partial from the streaming recognizer — replaces in place
   *  on each update, becomes a finalized line when `is_final` arrives. */
  pendingPartial: TranscriptLine | null;
  /** True once the first transcript (partial or final) has been received
   *  from the backend. Used to show a "Starting up…" state before the
   *  pipeline is actually producing output. */
  pipelineReady: boolean;
}

export type SpeakersStatus = 'idle' | 'loading' | 'ready' | 'error';

interface VoiceState {
  isRecording: boolean;
  currentSession: RecordingSession | null;
  segments: Segment[];
  clipInterval: ClipInterval;

  // Backend-backed speaker registry.
  speakers: Speaker[];
  speakersStatus: SpeakersStatus;
  speakersError: string | null;

  /** Internal — not serialised to localStorage */
  _ws: WebSocket | null;

  // Recording + segment CRUD (unchanged).
  startRecording: () => void;
  stopRecording: () => void;
  appendLiveTranscript: (text: string) => void;
  flushInterval: () => void;
  starSegment: (id: string) => void;
  unstarSegment: (id: string) => void;
  deleteSegment: (id: string) => void;
  setClipInterval: (minutes: ClipInterval) => void;

  // Speaker actions — all talk to the backend.
  fetchSpeakers: () => Promise<void>;
  createSpeaker: (input: { display_name: string; color: string }) => Promise<Speaker>;
  updateSpeaker: (id: string, patch: { display_name?: string; color?: string }) => Promise<void>;
  deleteSpeaker: (id: string) => Promise<void>;
}

const MAX_UNSTARRED = 30;
const STORAGE_KEY = 'actio-voice';

// Exported for unit testing
export function pruneSegments(segments: Segment[]): Segment[] {
  // segments are newest-first; keep all starred, keep at most MAX_UNSTARRED unstarred
  let unstarredCount = 0;
  return segments.filter((s) => {
    if (s.starred) return true;
    unstarredCount++;
    return unstarredCount <= MAX_UNSTARRED;
  });
}

interface PersistedVoiceData {
  segments: Segment[];
  clipInterval: ClipInterval;
}

function loadVoiceData(): PersistedVoiceData {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null');
    if (raw && typeof raw === 'object') {
      return {
        segments: Array.isArray(raw.segments) ? raw.segments : [],
        clipInterval: [1, 2, 5, 10, 30].includes(raw.clipInterval) ? raw.clipInterval : 5,
      };
    }
  } catch {
    /* fall through */
  }
  return { segments: [], clipInterval: 5 };
}

function saveVoiceData(segments: Segment[], clipInterval: ClipInterval) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ segments, clipInterval }));
}

const { segments: initialSegments, clipInterval: initialClipInterval } = loadVoiceData();

type StoreSet = (
  update: (s: VoiceState) => Partial<VoiceState> | VoiceState,
) => void;

// --- WS message handlers ---

/** `kind: "transcript"` — either a partial (replace in-progress line) or a
 *  final (append to lines, clear partial). Upserts by transcript_id. */
function handleTranscriptMessage(
  set: StoreSet,
  msg: {
    transcript_id?: string;
    text: string;
    start_ms?: number;
    end_ms?: number;
    is_final?: boolean;
    speaker_id?: string | null;
  },
) {
  if (!msg.text.trim()) return;
  const isFinal = msg.is_final ?? false;

  set((state) => {
    if (!state.currentSession) return state;
    const line: TranscriptLine = {
      id: msg.transcript_id || `local-${crypto.randomUUID()}`,
      text: msg.text,
      start_ms: msg.start_ms ?? 0,
      end_ms: msg.end_ms ?? 0,
      speaker_id: msg.speaker_id ?? null,
      is_final: isFinal,
    };

    if (!isFinal) {
      return {
        currentSession: {
          ...state.currentSession,
          pendingPartial: line,
          pipelineReady: true,
        },
      };
    }

    // Finalize: if a line with this id already exists (late re-broadcast
    // from a backfill path), upsert; otherwise append. Clear the partial.
    const lines = state.currentSession.lines;
    const existingIdx = lines.findIndex((l) => l.id === line.id);
    const nextLines =
      existingIdx >= 0
        ? lines.map((l, i) => (i === existingIdx ? { ...l, ...line } : l))
        : [...lines, line];

    return {
      currentSession: {
        ...state.currentSession,
        lines: nextLines,
        pendingPartial: null,
        pipelineReady: true,
      },
    };
  });
}

/** `kind: "speaker_resolved"` — the per-segment identification task has
 *  decided who was speaking during [start_ms, end_ms]. Fill in speaker_id
 *  on every line whose time range overlaps. */
function handleSpeakerResolved(
  set: StoreSet,
  msg: { start_ms?: number; end_ms?: number; speaker_id?: string | null },
) {
  const start = msg.start_ms;
  const end = msg.end_ms;
  const speaker = msg.speaker_id ?? null;
  if (typeof start !== 'number' || typeof end !== 'number') return;

  set((state) => {
    if (!state.currentSession) return state;
    let changed = false;
    const lines = state.currentSession.lines.map((line) => {
      // Overlap test: two ranges overlap if neither ends before the other
      // starts. Skip lines that already have a speaker — they're resolved.
      if (line.speaker_id !== null) return line;
      if (line.end_ms < start || line.start_ms > end) return line;
      changed = true;
      return { ...line, speaker_id: speaker };
    });
    if (!changed) return state;
    return {
      currentSession: { ...state.currentSession, lines },
    };
  });
}

export const useVoiceStore = create<VoiceState>((set, get) => ({
  isRecording: false,
  currentSession: null,
  segments: initialSegments,
  clipInterval: initialClipInterval,

  speakers: [],
  speakersStatus: 'idle',
  speakersError: null,

  _ws: null,

  startRecording: () => {
    // The backend starts microphone capture while at least one WebSocket
    // subscriber is attached. Closing this socket stops our recording session.
    const session: RecordingSession = {
      id: 'live',
      startedAt: new Date().toISOString(),
      lines: [],
      pendingPartial: null,
      pipelineReady: false,
    };
    set({ isRecording: true, currentSession: session });

    void getWsUrl('/ws')
      .then((url) => {
        if (!get().currentSession) return;
        const ws = new WebSocket(url);
        ws.onmessage = (event) => {
          try {
            const msg = JSON.parse(event.data);
            if (msg.kind === 'transcript' && typeof msg.text === 'string') {
              handleTranscriptMessage(set, msg);
            } else if (msg.kind === 'speaker_resolved') {
              handleSpeakerResolved(set, msg);
            }
          } catch { /* ignore malformed frames */ }
        };
        ws.onerror = (e) => console.warn('[Actio] WS error', e);
        set({ _ws: ws });
      })
      .catch((e) => console.warn('[Actio] WS discovery failed', e));
  },

  stopRecording: () => {
    const { currentSession, _ws } = get();
    _ws?.close();
    if (currentSession && currentSession.lines.length > 0) get().flushInterval();
    set({ isRecording: false, currentSession: null, _ws: null });
  },

  appendLiveTranscript: (text) => {
    // Mainly used by tests to simulate an ASR final. Creates a synthetic
    // line with no backend id so it won't collide with real transcripts.
    set((state) => {
      if (!state.currentSession) return state;
      const line: TranscriptLine = {
        id: `local-${crypto.randomUUID()}`,
        text,
        start_ms: 0,
        end_ms: 0,
        speaker_id: null,
        is_final: true,
      };
      return {
        currentSession: {
          ...state.currentSession,
          lines: [...state.currentSession.lines, line],
        },
      };
    });
  },

  flushInterval: () => {
    const { currentSession, segments, clipInterval } = get();
    if (!currentSession || currentSession.lines.length === 0) return;

    const text = currentSession.lines.map((l) => l.text).join(' ').trim();
    if (!text) return;

    const newSegment: Segment = {
      id: crypto.randomUUID(),
      sessionId: currentSession.id,
      text,
      createdAt: new Date().toISOString(),
      starred: false,
    };

    const next = pruneSegments([newSegment, ...segments]);
    saveVoiceData(next, clipInterval);
    set({
      segments: next,
      currentSession: { ...currentSession, lines: [] },
    });
  },

  starSegment: (id) => {
    set((state) => {
      const next = state.segments.map((s) => (s.id === id ? { ...s, starred: true } : s));
      saveVoiceData(next, state.clipInterval);
      return { segments: next };
    });
  },

  unstarSegment: (id) => {
    set((state) => {
      const mapped = state.segments.map((s) => (s.id === id ? { ...s, starred: false } : s));
      const next = pruneSegments(mapped);
      saveVoiceData(next, state.clipInterval);
      return { segments: next };
    });
  },

  deleteSegment: (id) => {
    set((state) => {
      const next = state.segments.filter((s) => s.id !== id);
      saveVoiceData(next, state.clipInterval);
      return { segments: next };
    });
  },

  setClipInterval: (minutes) => {
    set((state) => {
      saveVoiceData(state.segments, minutes);
      return { clipInterval: minutes };
    });
  },

  // --- Speaker actions ---

  fetchSpeakers: async () => {
    set({ speakersStatus: 'loading', speakersError: null });
    try {
      const list = await speakerApi.listSpeakers();
      set({ speakers: list, speakersStatus: 'ready' });
    } catch (e) {
      set({ speakersStatus: 'error', speakersError: (e as Error).message });
    }
  },

  createSpeaker: async (input) => {
    const s = await speakerApi.createSpeaker(input);
    set((state) => ({ speakers: [s, ...state.speakers] }));
    return s;
  },

  updateSpeaker: async (id, patch) => {
    // Optimistic — fall back to re-fetch on failure.
    const prev = get().speakers;
    set({
      speakers: prev.map((s) =>
        s.id === id ? { ...s, ...(patch.display_name ? { display_name: patch.display_name } : {}), ...(patch.color ? { color: patch.color } : {}) } : s,
      ),
    });
    try {
      const updated = await speakerApi.updateSpeaker(id, patch);
      set((state) => ({
        speakers: state.speakers.map((s) => (s.id === id ? updated : s)),
      }));
    } catch (e) {
      set({ speakers: prev });
      throw e;
    }
  },

  deleteSpeaker: async (id) => {
    const prev = get().speakers;
    set({ speakers: prev.filter((s) => s.id !== id) });
    try {
      await speakerApi.deleteSpeaker(id);
    } catch (e) {
      set({ speakers: prev });
      throw e;
    }
  },
}));
