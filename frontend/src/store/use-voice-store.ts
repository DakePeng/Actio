import { create } from 'zustand';
import type { Segment, Person } from '../types';

export type ClipInterval = 1 | 2 | 5 | 10 | 30;

const WS_BASE = 'ws://127.0.0.1:3000';

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

interface VoiceState {
  isRecording: boolean;
  currentSession: RecordingSession | null;
  segments: Segment[];
  people: Person[];
  clipInterval: ClipInterval;
  /** Internal — not serialised to localStorage */
  _ws: WebSocket | null;

  startRecording: () => void;
  stopRecording: () => void;
  appendLiveTranscript: (text: string) => void;
  flushInterval: () => void;
  starSegment: (id: string) => void;
  unstarSegment: (id: string) => void;
  deleteSegment: (id: string) => void;
  addPerson: (name: string, color: string) => void;
  updatePerson: (id: string, updates: { name?: string; color?: string }) => void;
  deletePerson: (id: string) => void;
  setClipInterval: (minutes: ClipInterval) => void;
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

function loadVoiceData(): { segments: Segment[]; people: Person[]; clipInterval: ClipInterval } {
  try {
    return (
      JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null') ?? {
        segments: [],
        people: [],
        clipInterval: 5,
      }
    );
  } catch {
    return { segments: [], people: [], clipInterval: 5 };
  }
}

function saveVoiceData(segments: Segment[], people: Person[], clipInterval: ClipInterval) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify({ segments, people, clipInterval }));
}

const { segments: initialSegments, people: initialPeople, clipInterval: initialClipInterval } =
  loadVoiceData();

export const useVoiceStore = create<VoiceState>((set, get) => ({
  isRecording: false,
  currentSession: null,
  segments: initialSegments,
  people: initialPeople,
  clipInterval: initialClipInterval,
  _ws: null,

  startRecording: () => {
    // The backend runs an always-on inference pipeline. Recording is just
    // "subscribe to the live transcript stream" — no session creation, no
    // backend state change. Closing the WebSocket detaches us; the pipeline
    // keeps running for other consumers.
    const session: RecordingSession = {
      id: 'live',
      startedAt: new Date().toISOString(),
      liveTranscript: '',
      pendingPartial: '',
      pipelineReady: false,
    };
    set({ isRecording: true, currentSession: session });

    const ws = new WebSocket(`${WS_BASE}/ws`);
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
    const { currentSession, segments, people, clipInterval } = get();
    if (!currentSession || !currentSession.liveTranscript.trim()) return;

    const newSegment: Segment = {
      id: crypto.randomUUID(),
      sessionId: currentSession.id,
      text: currentSession.liveTranscript.trim(),
      createdAt: new Date().toISOString(),
      starred: false,
    };

    const next = pruneSegments([newSegment, ...segments]);
    saveVoiceData(next, people, clipInterval);
    set({
      segments: next,
      currentSession: { ...currentSession, liveTranscript: '' },
    });
  },

  starSegment: (id) => {
    set((state) => {
      const next = state.segments.map((s) => (s.id === id ? { ...s, starred: true } : s));
      saveVoiceData(next, state.people, state.clipInterval);
      return { segments: next };
    });
  },

  unstarSegment: (id) => {
    set((state) => {
      const mapped = state.segments.map((s) => (s.id === id ? { ...s, starred: false } : s));
      const next = pruneSegments(mapped);
      saveVoiceData(next, state.people, state.clipInterval);
      return { segments: next };
    });
  },

  deleteSegment: (id) => {
    set((state) => {
      const next = state.segments.filter((s) => s.id !== id);
      saveVoiceData(next, state.people, state.clipInterval);
      return { segments: next };
    });
  },

  addPerson: (name, color) => {
    set((state) => {
      const person: Person = {
        id: crypto.randomUUID(),
        name,
        color,
        createdAt: new Date().toISOString(),
      };
      const next = [...state.people, person];
      saveVoiceData(state.segments, next, state.clipInterval);
      return { people: next };
    });
  },

  updatePerson: (id, updates) => {
    set((state) => {
      const next = state.people.map((p) => (p.id === id ? { ...p, ...updates } : p));
      saveVoiceData(state.segments, next, state.clipInterval);
      return { people: next };
    });
  },

  deletePerson: (id) => {
    set((state) => {
      const next = state.people.filter((p) => p.id !== id);
      saveVoiceData(state.segments, next, state.clipInterval);
      return { people: next };
    });
  },

  setClipInterval: (minutes) => {
    set((state) => {
      saveVoiceData(state.segments, state.people, minutes);
      return { clipInterval: minutes };
    });
  },
}));
