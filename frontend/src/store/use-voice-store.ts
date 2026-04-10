import { create } from 'zustand';
import type { Segment, Person } from '../types';

export type ClipInterval = 1 | 2 | 5 | 10 | 30;

interface RecordingSession {
  id: string;
  startedAt: string;
  liveTranscript: string;
}

interface VoiceState {
  isRecording: boolean;
  currentSession: RecordingSession | null;
  segments: Segment[];
  people: Person[];
  clipInterval: ClipInterval;

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

  startRecording: () => {
    const session: RecordingSession = {
      id: crypto.randomUUID(),
      startedAt: new Date().toISOString(),
      liveTranscript: '',
    };
    set({ isRecording: true, currentSession: session });
  },

  stopRecording: () => {
    const { currentSession } = get();
    if (currentSession?.liveTranscript.trim()) {
      get().flushInterval();
    }
    set({ isRecording: false, currentSession: null });
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
