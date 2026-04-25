import { create } from 'zustand';
import type { Segment } from '../types';
import type { Speaker } from '../types/speaker';
import { getWsUrl } from '../api/backend-url';
import * as speakerApi from '../api/speakers';
import { translateLines, LlmDisabledError } from '../api/translate';

export type ClipInterval = 1 | 2 | 5 | 10 | 30;

/** A single finalized transcript line. `speaker_id` starts null and fills
 *  in when the backend emits a `speaker_resolved` WS event whose time
 *  range overlaps this line's [start_ms, end_ms]. `resolved` flips true
 *  when that event arrives — even if the match came back as Unknown
 *  (speaker_id still null) — so the UI can drop the "Identifying…"
 *  placeholder and render "Unknown" instead. */
export interface TranscriptLine {
  id: string;
  text: string;
  start_ms: number;
  end_ms: number;
  speaker_id: string | null;
  resolved: boolean;
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

  translation: {
    enabled: boolean;
    targetLang: string;
    byLineId: Record<string, { status: 'pending' | 'done' | 'error'; text?: string }>;
  };

  setTranslationEnabled: (enabled: boolean) => Promise<void>;
  setTranslationTargetLang: (lang: string) => Promise<void>;
  queueLineForTranslation: (lineId: string) => void;
  flushTranslationBatch: () => Promise<void>;
  retryTranslationLine: (lineId: string) => void;
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

const TRANSLATE_LANG_KEY = 'actio-translate-target';

function loadTranslateTarget(): string {
  try {
    return localStorage.getItem(TRANSLATE_LANG_KEY) ?? 'en';
  } catch {
    return 'en';
  }
}

function saveTranslateTarget(lang: string): void {
  try {
    localStorage.setItem(TRANSLATE_LANG_KEY, lang);
  } catch { /* private mode etc. */ }
}

// Single-flight guard for translation flushes. The 3s interval can fire
// while a prior flush is still awaiting the LLM response — without this
// guard, a second flush would re-send the same pending ids and the two
// responses would race into the same byLineId.
let flushInFlight = false;

const { segments: initialSegments, clipInterval: initialClipInterval } = loadVoiceData();

type StoreSet = (
  update: (s: VoiceState) => Partial<VoiceState> | VoiceState,
) => void;

// --- WS message handlers ---

/** Buffer for `speaker_resolved` events that arrived before any line whose
 *  midpoint falls inside their [start_ms, end_ms] range. Short utterances
 *  (enrollment passages, especially) often finish embedding identification
 *  before the ASR final lands, so a naïve "walk current lines" pass finds
 *  nothing and the event was silently dropped — leaving the bubble stuck in
 *  "Identifying…". We replay this buffer whenever a transcript finalizes.
 *
 *  Entries age out after PENDING_TTL_MS so a malformed or stray event from a
 *  prior session doesn't linger forever. Capped at PENDING_MAX to bound
 *  memory under pathological backend behavior. */
interface PendingResolution {
  start_ms: number;
  end_ms: number;
  speaker_id: string | null;
  received_at: number;
}
const pendingResolutions: PendingResolution[] = [];
const PENDING_TTL_MS = 30_000;
const PENDING_MAX = 64;

function prunePendingResolutions(now: number) {
  const cutoff = now - PENDING_TTL_MS;
  let write = 0;
  for (let read = 0; read < pendingResolutions.length; read++) {
    if (pendingResolutions[read].received_at >= cutoff) {
      pendingResolutions[write++] = pendingResolutions[read];
    }
  }
  pendingResolutions.length = write;
}

function clearPendingResolutionsForSession() {
  pendingResolutions.length = 0;
}

/** Apply any buffered `speaker_resolved` events whose range now covers a
 *  newly-finalized line's midpoint. Mutates the input array of lines and
 *  returns a fresh array if anything changed, else the original. */
function applyPendingResolutions(lines: TranscriptLine[]): TranscriptLine[] {
  if (pendingResolutions.length === 0) return lines;
  let next: TranscriptLine[] | null = null;
  const consumedIndices = new Set<number>();
  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];
    if (line.resolved) continue;
    const mid = (line.start_ms + line.end_ms) / 2;
    for (let pi = 0; pi < pendingResolutions.length; pi++) {
      if (consumedIndices.has(pi)) continue;
      const p = pendingResolutions[pi];
      if (mid < p.start_ms || mid > p.end_ms) continue;
      next = next ?? [...lines];
      next[li] = { ...line, speaker_id: p.speaker_id, resolved: true };
      consumedIndices.add(pi);
      break;
    }
  }
  if (consumedIndices.size > 0) {
    let write = 0;
    for (let i = 0; i < pendingResolutions.length; i++) {
      if (!consumedIndices.has(i)) pendingResolutions[write++] = pendingResolutions[i];
    }
    pendingResolutions.length = write;
  }
  return next ?? lines;
}

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

  const id = msg.transcript_id || `local-${crypto.randomUUID()}`;

  set((state) => {
    if (!state.currentSession) return state;
    const speakerId = msg.speaker_id ?? null;
    const line: TranscriptLine = {
      id,
      text: msg.text,
      start_ms: msg.start_ms ?? 0,
      end_ms: msg.end_ms ?? 0,
      speaker_id: speakerId,
      resolved: speakerId !== null,
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
    const nextLinesRaw =
      existingIdx >= 0
        ? lines.map((l, i) => (i === existingIdx ? { ...l, ...line } : l))
        : [...lines, line];
    // Replay buffered speaker_resolved events whose window now contains
    // this line's midpoint.
    prunePendingResolutions(Date.now());
    const nextLines = applyPendingResolutions(nextLinesRaw);

    return {
      currentSession: {
        ...state.currentSession,
        lines: nextLines,
        pendingPartial: null,
        pipelineReady: true,
      },
    };
  });

  // Queue the new final for translation if the toggle is on.
  if (isFinal) {
    const ts = useVoiceStore.getState();
    if (ts.translation.enabled) {
      ts.queueLineForTranslation(id);
    }
  }
}

/** `kind: "speaker_resolved"` — the per-segment identification task has
 *  decided who was speaking during [start_ms, end_ms]. Fill in speaker_id
 *  on every line whose time range overlaps. If no line matches yet (the
 *  event outraced the ASR final), buffer it so the next finalize pass can
 *  claim it. */
function handleSpeakerResolved(
  set: StoreSet,
  msg: { start_ms?: number; end_ms?: number; speaker_id?: string | null },
) {
  const start = msg.start_ms;
  const end = msg.end_ms;
  const speaker = msg.speaker_id ?? null;
  if (typeof start !== 'number' || typeof end !== 'number') return;

  let claimed = false;
  set((state) => {
    if (!state.currentSession) return state;
    let changed = false;
    const lines = state.currentSession.lines.map((line) => {
      // Midpoint-in-segment test: attribute a line to whichever segment
      // contains its center. Any-overlap caused boundary lines to get
      // labeled by the FIRST arriving neighbor segment, which is wrong
      // when speakers switch mid-line. Skip lines already resolved — we
      // don't want to overwrite a known speaker with a later event.
      if (line.resolved) return line;
      const mid = (line.start_ms + line.end_ms) / 2;
      if (mid < start || mid > end) return line;
      changed = true;
      claimed = true;
      return { ...line, speaker_id: speaker, resolved: true };
    });
    if (!changed) return state;
    return {
      currentSession: { ...state.currentSession, lines },
    };
  });

  if (!claimed) {
    const now = Date.now();
    prunePendingResolutions(now);
    pendingResolutions.push({
      start_ms: start,
      end_ms: end,
      speaker_id: speaker,
      received_at: now,
    });
    if (pendingResolutions.length > PENDING_MAX) {
      pendingResolutions.splice(0, pendingResolutions.length - PENDING_MAX);
    }
  }
}

export const useVoiceStore = create<VoiceState>((set, get) => ({
  isRecording: false,
  currentSession: null,
  segments: initialSegments,
  clipInterval: initialClipInterval,

  speakers: [],
  speakersStatus: 'idle',
  speakersError: null,

  translation: {
    enabled: false,
    targetLang: loadTranslateTarget(),
    byLineId: {},
  },

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
    clearPendingResolutionsForSession();
    set((state) => ({
      isRecording: false,
      currentSession: null,
      _ws: null,
      translation: { ...state.translation, byLineId: {} },
    }));
  },

  setTranslationEnabled: async (enabled) => {
    set((state) => ({ translation: { ...state.translation, enabled } }));
    if (!enabled) return;
    const { currentSession, translation } = get();
    if (!currentSession) return;
    const missing = currentSession.lines
      .filter((l) => l.is_final && !translation.byLineId[l.id])
      .map((l) => ({ id: l.id, text: l.text }));
    if (missing.length === 0) return;
    set((state) => {
      const next = { ...state.translation.byLineId };
      for (const m of missing) next[m.id] = { status: 'pending' };
      return { translation: { ...state.translation, byLineId: next } };
    });
    await get().flushTranslationBatch();
  },

  setTranslationTargetLang: async (lang) => {
    saveTranslateTarget(lang);
    set((state) => ({
      translation: { ...state.translation, targetLang: lang, byLineId: {} },
    }));
    if (get().translation.enabled) {
      const { currentSession } = get();
      if (currentSession) {
        const missing = currentSession.lines
          .filter((l) => l.is_final)
          .map((l) => ({ id: l.id, text: l.text }));
        if (missing.length > 0) {
          set((state) => {
            const next: Record<string, { status: 'pending' | 'done' | 'error'; text?: string }> = {};
            for (const m of missing) next[m.id] = { status: 'pending' };
            return { translation: { ...state.translation, byLineId: next } };
          });
          await get().flushTranslationBatch();
        }
      }
    }
  },

  queueLineForTranslation: (lineId) => {
    if (!get().translation.enabled) return;
    set((state) => {
      if (state.translation.byLineId[lineId]) return state;
      return {
        translation: {
          ...state.translation,
          byLineId: { ...state.translation.byLineId, [lineId]: { status: 'pending' } },
        },
      };
    });
  },

  flushTranslationBatch: async () => {
    if (flushInFlight) return;
    const { translation, currentSession } = get();
    if (!translation.enabled || !currentSession) return;
    const pending = Object.entries(translation.byLineId)
      .filter(([, v]) => v.status === 'pending')
      .map(([id]) => id);
    if (pending.length === 0) return;
    const idToText = new Map(currentSession.lines.map((l) => [l.id, l.text] as const));
    const lines = pending
      .map((id) => ({ id, text: idToText.get(id) ?? '' }))
      .filter((l) => l.text);
    if (lines.length === 0) return;
    flushInFlight = true;
    try {
      const out = await translateLines(translation.targetLang, lines);
      const post = get();
      // If the user muted or toggled off mid-flush, the byLineId we'd be
      // writing to has either been cleared or is for a closed session —
      // dropping the response is correct.
      if (!post.translation.enabled || !post.currentSession) return;
      const returnedIds = new Set(out.map((t) => t.id));
      set((state) => {
        const next = { ...state.translation.byLineId };
        for (const t of out) next[t.id] = { status: 'done', text: t.text };
        // Anything we asked about that the LLM didn't return stays as
        // 'error' so the UI shows a retry link instead of an indefinite
        // 'translating' placeholder.
        for (const id of lines.map((l) => l.id)) {
          if (!returnedIds.has(id) && next[id]?.status === 'pending') {
            next[id] = { status: 'error' };
          }
        }
        return { translation: { ...state.translation, byLineId: next } };
      });
    } catch (e) {
      if (e instanceof LlmDisabledError) {
        set({ translation: { ...get().translation, enabled: false, byLineId: {} } });
        return;
      }
      const post = get();
      if (!post.translation.enabled || !post.currentSession) return;
      const askedIds = new Set(lines.map((l) => l.id));
      set((state) => {
        const next = { ...state.translation.byLineId };
        for (const id of askedIds) {
          if (next[id]?.status === 'pending') next[id] = { status: 'error' };
        }
        return { translation: { ...state.translation, byLineId: next } };
      });
    } finally {
      flushInFlight = false;
    }
  },

  retryTranslationLine: (lineId) => {
    set((state) => ({
      translation: {
        ...state.translation,
        byLineId: { ...state.translation.byLineId, [lineId]: { status: 'pending' } },
      },
    }));
    void get().flushTranslationBatch();
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
        resolved: false,
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
