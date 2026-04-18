import { create } from 'zustand';
import type { Segment } from '../types';
import type {
  Speaker,
  UnknownSegment,
  AssignTarget,
} from '../types/speaker';
import { getWsUrl } from '../api/backend-url';
import * as speakerApi from '../api/speakers';

export type ClipInterval = 1 | 2 | 5 | 10 | 30;

interface RecordingSession {
  id: string;
  startedAt: string;
  /** Committed (final) transcript text */
  liveTranscript: string;
  /** Current in-progress partial from ASR */
  pendingPartial: string;
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

  // Retroactive tagging — unknown segments to assign.
  unknowns: UnknownSegment[];
  /** Client-side soft-hide — survives session lifetime but not reload. */
  dismissedUnknowns: Set<string>;

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

  // Unknown-segment actions.
  fetchUnknowns: () => Promise<void>;
  assignSegment: (segmentId: string, target: AssignTarget) => Promise<void>;
  dismissUnknown: (segmentId: string) => void;
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

export const useVoiceStore = create<VoiceState>((set, get) => ({
  isRecording: false,
  currentSession: null,
  segments: initialSegments,
  clipInterval: initialClipInterval,

  speakers: [],
  speakersStatus: 'idle',
  speakersError: null,

  unknowns: [],
  dismissedUnknowns: new Set<string>(),

  _ws: null,

  startRecording: () => {
    // The backend starts microphone capture while at least one WebSocket
    // subscriber is attached. Closing this socket stops our recording session.
    const session: RecordingSession = {
      id: 'live',
      startedAt: new Date().toISOString(),
      liveTranscript: '',
      pendingPartial: '',
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
            if (msg.kind === 'transcript' && msg.text) {
              if (msg.is_final) {
                // Commit: append finalized text, clear pending partial
                set((state) => {
                  if (!state.currentSession) return state;
                  const prev = state.currentSession.liveTranscript;
                  return {
                    currentSession: {
                      ...state.currentSession,
                      liveTranscript: prev ? `${prev} ${msg.text}` : msg.text,
                      pendingPartial: '',
                      pipelineReady: true,
                    },
                  };
                });
              } else {
                // Partial: replace in-progress text (don't append)
                set((state) => {
                  if (!state.currentSession) return state;
                  return {
                    currentSession: {
                      ...state.currentSession,
                      pendingPartial: msg.text,
                      pipelineReady: true,
                    },
                  };
                });
              }
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
    if (currentSession?.liveTranscript.trim()) get().flushInterval();
    set({ isRecording: false, currentSession: null, _ws: null });
  },

  appendLiveTranscript: (text) => {
    set((state) => {
      if (!state.currentSession) return state;
      const prev = state.currentSession.liveTranscript;
      return {
        currentSession: {
          ...state.currentSession,
          liveTranscript: prev ? `${prev} ${text}` : text,
        },
      };
    });
  },

  flushInterval: () => {
    const { currentSession, segments, clipInterval } = get();
    if (!currentSession || !currentSession.liveTranscript.trim()) return;

    const newSegment: Segment = {
      id: crypto.randomUUID(),
      sessionId: currentSession.id,
      text: currentSession.liveTranscript.trim(),
      createdAt: new Date().toISOString(),
      starred: false,
    };

    const next = pruneSegments([newSegment, ...segments]);
    saveVoiceData(next, clipInterval);
    set({
      segments: next,
      currentSession: { ...currentSession, liveTranscript: '' },
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

  // --- Unknown segments ---

  fetchUnknowns: async () => {
    const list = await speakerApi.listUnknowns(50);
    const dismissed = get().dismissedUnknowns;
    set({ unknowns: list.filter((u) => !dismissed.has(u.segment_id)) });
  },

  assignSegment: async (segmentId, target) => {
    const prev = get().unknowns;
    // Optimistic removal.
    set({ unknowns: prev.filter((u) => u.segment_id !== segmentId) });
    try {
      await speakerApi.assignSegment(segmentId, target);
      // Refresh speakers if we just created a new one inline.
      if ('new_speaker' in target) {
        void get().fetchSpeakers();
      }
    } catch (e) {
      set({ unknowns: prev });
      throw e;
    }
  },

  dismissUnknown: (segmentId) => {
    set((state) => {
      const dismissed = new Set(state.dismissedUnknowns);
      dismissed.add(segmentId);
      return {
        dismissedUnknowns: dismissed,
        unknowns: state.unknowns.filter((u) => u.segment_id !== segmentId),
      };
    });
  },
}));
