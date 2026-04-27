// This store assumes a single app instance per process. Module-level
// state below (`pendingResolutions`, `flushInFlight`) is shared across
// all consumers — fine for the desktop shell that only ever runs one
// LiveTab at a time. If we ever support multiple windows / tabs of
// the same backend, both need to move into the store or be keyed by
// session id.
import { create } from 'zustand';
import type { Segment } from '../types';
import type { Speaker } from '../types/speaker';
import { getApiBaseUrl, getWsUrl } from '../api/backend-url';
import { DEV_TENANT_ID } from '../api/actio-api';
import * as speakerApi from '../api/speakers';
import { translateLines, LlmDisabledError } from '../api/translate';

export type ClipInterval = 1 | 2 | 5 | 10 | 30;

/** A translation slice entry per transcript line.
 *  `attempts` counts how many times the LLM has been asked about this
 *  line. Bumped on each batch failure / dropped id; once it exceeds
 *  MAX_AUTO_RETRIES the entry stays 'error' until the user clicks
 *  retry (which resets attempts to 0). */
export type TranslationEntry = {
  status: 'pending' | 'done' | 'error';
  text?: string;
  attempts?: number;
};

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

  /** EMA-smoothed mic RMS, sampled by the backend at ~15Hz and pushed
   *  on the WebSocket as `{kind: "audio_level", rms: …}`. Drives the
   *  voice-wave visualisation. Reset to 0 on stopRecording. */
  audioLevel: number;

  // Recording + segment CRUD (unchanged).
  startRecording: () => void;
  stopRecording: () => void;
  appendLiveTranscript: (text: string) => void;
  flushInterval: () => void;
  /** Pull processed clips from the backend `audio_clips` table (the
   *  always-listening batch pipeline) and merge them into `segments`.
   *  Idempotent: existing segment ids are preserved with their starred
   *  state, only new backend clips get appended. */
  loadBackendClips: () => Promise<void>;
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
    byLineId: Record<string, TranslationEntry>;
    /** Per-language translation cache, keyed by source text. Populated
     *  on every successful LLM response. Lets target-lang changes reuse
     *  prior translations instantly instead of re-billing the LLM —
     *  flipping `en → zh-CN → en` should not re-translate. Bounded
     *  to MAX_CACHE_ENTRIES_PER_LANG and cleared on stopRecording. */
    cache: Record<string, Record<string, string>>;
  };

  setTranslationEnabled: (enabled: boolean) => Promise<void>;
  setTranslationTargetLang: (lang: string) => Promise<void>;
  queueLineForTranslation: (lineId: string) => void;
  flushTranslationBatch: () => Promise<void>;
  retryTranslationLine: (lineId: string) => void;
}

const MAX_UNSTARRED = 30;
const STORAGE_KEY = 'actio-voice';

/** Whitespace + the most common terminal punctuation across Latin and
 *  CJK. Used to test whether a finalized ASR line carries any actual
 *  content — see `isMeaningfulFinal`. */
const NOISE_STRIP_RE = /[\s.,!?;:'"`()[\]{}。、！？，；：·…—\-]+/gu;

/** Drop ASR finals that, after stripping whitespace and punctuation,
 *  carry fewer than 2 graphemes of content. This catches the breath /
 *  click / tail-of-partial mishearings ("そ", "。", "u") that the
 *  recognizer routinely emits as standalone finals on quiet windows.
 *
 *  Single-grapheme affirmations like "嗯" or "好" are dropped too —
 *  acceptable trade for live transcripts; the user can scan past
 *  these but visually they're indistinguishable from noise. If we
 *  ever need to keep them, raise the threshold to 1 with an
 *  additional duration guard. */
export function isMeaningfulFinal(text: string): boolean {
  const stripped = text.replace(NOISE_STRIP_RE, '');
  // Array.from gives codepoint count, not UTF-16 unit count — important
  // for surrogate-pair scripts (some kanji extensions, emoji).
  return Array.from(stripped).length >= 2;
}

/** Cheap script-based check for whether a transcript line is already in
 *  the user's target language. Counts letter-like graphemes by Unicode
 *  block and decides by majority. The point is to skip the LLM entirely
 *  for already-target lines — the system prompt would just echo them
 *  back after a 5–15s round-trip. We err toward FALSE (translate) when
 *  ambiguous; a wasted LLM call is better than a missed translation.
 *
 *  Latin targets (en/es/fr/de) share the same script bucket. We don't
 *  try to distinguish English from Spanish — the LLM round-trip would
 *  catch that case anyway and the prompt instructs it to passthrough. */
const CJK_UNIFIED_RE = /[一-鿿㐀-䶿]/;
const HIRAGANA_KATAKANA_RE = /[぀-ゟ゠-ヿ]/;
const LATIN_LETTER_RE = /[A-Za-zÀ-ɏ]/;

export function looksLikeTargetLang(text: string, targetLang: string): boolean {
  let total = 0;
  let cjkUnified = 0;
  let kana = 0;
  let latin = 0;
  for (const ch of text) {
    if (CJK_UNIFIED_RE.test(ch)) {
      cjkUnified++;
      total++;
    } else if (HIRAGANA_KATAKANA_RE.test(ch)) {
      kana++;
      total++;
    } else if (LATIN_LETTER_RE.test(ch)) {
      latin++;
      total++;
    }
    // Punctuation, digits, whitespace: ignored.
  }
  if (total < 3) return false; // Too short to classify confidently — translate.
  const threshold = 0.7;
  switch (targetLang) {
    case 'en':
    case 'es':
    case 'fr':
    case 'de':
      return latin / total >= threshold;
    case 'zh-CN':
      // Predominantly CJK and not actually Japanese.
      return cjkUnified / total >= threshold && kana === 0;
    case 'ja':
      // Japanese requires kana; pure-kanji could be zh-CN, fall through to translate.
      return kana > 0 && (kana + cjkUnified) / total >= threshold;
    default:
      return false;
  }
}

/** Decide what entry to store for a newly-queued line. Three outcomes:
 *  - same script as target → instant `done` with text === source
 *    (LiveTranscript suppresses the duplicate annotation).
 *  - cache hit for prior translation → instant `done` with cached text.
 *  - otherwise → `pending`, awaits the next flush.  */
type ClassifiedEntry =
  | { status: 'pending' }
  | { status: 'done'; text: string };

function classifyLine(
  text: string,
  targetLang: string,
  cache: Record<string, Record<string, string>>,
): ClassifiedEntry {
  if (looksLikeTargetLang(text, targetLang)) {
    return { status: 'done', text };
  }
  const cached = cache[targetLang]?.[text];
  if (cached !== undefined) {
    return { status: 'done', text: cached };
  }
  return { status: 'pending' };
}

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

// Cap on lines per translation request. qwen3.5-2b on CPU is slow
// (~50 tokens/sec); a 5-line batch ran 20s in production. With a 3s
// flush tick, smaller batches mean translations trickle in instead of
// landing all-at-once after a long blank period — better perceived
// latency. Excess pending lines wait for the next tick.
const MAX_LINES_PER_BATCH = 4;

// How many times we silently re-queue a line after the LLM drops it
// (whole batch parse error, network blip, id missing from response)
// before showing the user-facing 'error' retry button. Local LLMs go
// transient on degenerate responses (repetition loops, code-fence
// hallucinations), so a couple of free retries clears most cases
// without user intervention. The user's manual retry resets the
// counter.
const MAX_AUTO_RETRIES = 2;

/** Soft cap on per-language cache entries. JS objects iterate in
 *  insertion order, so trimming the oldest keys when the bucket grows
 *  past this cap keeps memory bounded in long sessions without breaking
 *  the lang-flip-back-and-forth UX (recent lines stay cached). */
const MAX_CACHE_ENTRIES_PER_LANG = 200;

function trimCacheBucket(
  bucket: Record<string, string>,
): Record<string, string> {
  const keys = Object.keys(bucket);
  if (keys.length <= MAX_CACHE_ENTRIES_PER_LANG) return bucket;
  const overflow = keys.length - MAX_CACHE_ENTRIES_PER_LANG;
  const trimmed: Record<string, string> = {};
  for (let i = overflow; i < keys.length; i++) {
    const k = keys[i]!;
    trimmed[k] = bucket[k]!;
  }
  return trimmed;
}

/** Increment the attempt counter on a previously-pending entry. While
 *  attempts < MAX_AUTO_RETRIES the entry stays 'pending' so the next
 *  flush tick re-sends it; once exceeded, it becomes 'error' and waits
 *  for a manual retry. */
function bumpRetry(
  entry: TranslationEntry | undefined,
): { status: 'pending' | 'error'; attempts: number } {
  const attempts = (entry?.attempts ?? 0) + 1;
  if (attempts > MAX_AUTO_RETRIES) {
    return { status: 'error', attempts };
  }
  return { status: 'pending', attempts };
}

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

/** Test-only helpers. The `pendingResolutions` array is module-state, so
 *  unit tests need a way to seed it (without going through the WebSocket /
 *  speaker_resolved handler) and to reset it between cases. Marked with
 *  `__` so consumers know not to depend on these. */
export function __pushPendingResolutionForTest(p: {
  start_ms: number;
  end_ms: number;
  speaker_id: string | null;
  received_at?: number;
}): void {
  pendingResolutions.push({
    start_ms: p.start_ms,
    end_ms: p.end_ms,
    speaker_id: p.speaker_id,
    received_at: p.received_at ?? Date.now(),
  });
}
export function __resetPendingResolutionsForTest(): void {
  pendingResolutions.length = 0;
}
export function __pendingResolutionsCountForTest(): number {
  return pendingResolutions.length;
}

/** Apply any buffered `speaker_resolved` events whose range now covers a
 *  newly-finalized line's midpoint. Mutates the input array of lines and
 *  returns a fresh array if anything changed, else the original.
 *
 *  Exported for test use (see `use-voice-store.resolutions.test.ts`); the
 *  production call site is inside `handleTranscriptMessage`. */
export function applyPendingResolutions(lines: TranscriptLine[]): TranscriptLine[] {
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

  // Drop ASR noise on finals (single-char fragments, pure punctuation).
  // These come from breath / clicks / partial-tail mishearings and add
  // pure visual noise to the transcript. Partials are left alone — they
  // either grow into something substantial or get replaced.
  if (isFinal && !isMeaningfulFinal(msg.text)) {
    // Clear any pending partial too, otherwise a stale "in progress"
    // bubble can hang around for the dropped final.
    set((state) => {
      if (!state.currentSession?.pendingPartial) return state;
      return {
        currentSession: { ...state.currentSession, pendingPartial: null },
      };
    });
    return;
  }

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
    cache: {},
  },

  _ws: null,

  audioLevel: 0,

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
            } else if (msg.kind === 'audio_level' && typeof msg.rms === 'number') {
              // Hot-path frame at ~15Hz; keep this lean.
              set({ audioLevel: msg.rms });
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
      audioLevel: 0,
      translation: { ...state.translation, byLineId: {}, cache: {} },
    }));
  },

  setTranslationEnabled: async (enabled) => {
    set((state) => ({ translation: { ...state.translation, enabled } }));
    if (!enabled) return;
    const { currentSession, translation } = get();
    if (!currentSession) return;
    const missing = currentSession.lines
      .filter((l) => l.is_final && !translation.byLineId[l.id]);
    if (missing.length === 0) return;
    set((state) => {
      const next = { ...state.translation.byLineId };
      for (const l of missing) {
        next[l.id] = classifyLine(l.text, state.translation.targetLang, state.translation.cache);
      }
      return { translation: { ...state.translation, byLineId: next } };
    });
    await get().flushTranslationBatch();
  },

  setTranslationTargetLang: async (lang) => {
    saveTranslateTarget(lang);
    // Clear byLineId for the new target lang, but keep `cache` —
    // translations for the OLD lang stay accessible if the user flips
    // back. classifyLine below repopulates byLineId from `cache` where
    // possible.
    set((state) => ({
      translation: { ...state.translation, targetLang: lang, byLineId: {} },
    }));
    if (!get().translation.enabled) return;
    const { currentSession } = get();
    if (!currentSession) return;
    const finals = currentSession.lines.filter((l) => l.is_final);
    if (finals.length === 0) return;
    set((state) => {
      const next: VoiceState['translation']['byLineId'] = {};
      for (const l of finals) {
        next[l.id] = classifyLine(l.text, lang, state.translation.cache);
      }
      return { translation: { ...state.translation, byLineId: next } };
    });
    await get().flushTranslationBatch();
  },

  queueLineForTranslation: (lineId) => {
    if (!get().translation.enabled) return;
    const session = get().currentSession;
    if (!session) return;
    const line = session.lines.find((l) => l.id === lineId);
    if (!line) return;
    set((state) => {
      if (state.translation.byLineId[lineId]) return state;
      return {
        translation: {
          ...state.translation,
          byLineId: {
            ...state.translation.byLineId,
            [lineId]: classifyLine(line.text, state.translation.targetLang, state.translation.cache),
          },
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
      .map(([id]) => id)
      .slice(0, MAX_LINES_PER_BATCH);
    if (pending.length === 0) return;
    const idToText = new Map(currentSession.lines.map((l) => [l.id, l.text] as const));
    const lines = pending
      .map((id) => ({ id, text: idToText.get(id) ?? '' }))
      .filter((l) => l.text);
    if (lines.length === 0) return;
    const flushLang = translation.targetLang;
    flushInFlight = true;
    try {
      const out = await translateLines(flushLang, lines);
      const post = get();
      // If the user muted, toggled off, or switched target language
      // mid-flush, this response is for a stale request — dropping it
      // is correct. setTranslationTargetLang already cleared byLineId
      // and re-pended the lines for the new language.
      if (
        !post.translation.enabled ||
        !post.currentSession ||
        post.translation.targetLang !== flushLang
      ) return;
      const returnedIds = new Set(out.map((t) => t.id));
      set((state) => {
        const next = { ...state.translation.byLineId };
        const nextCache = { ...state.translation.cache };
        const langBucket = { ...(nextCache[flushLang] ?? {}) };
        for (const t of out) {
          // Empty / whitespace-only text from the LLM means the model
          // intentionally returned nothing — not a transient failure.
          // Skip auto-retry; surface the retry UI immediately so the
          // user can intervene. (Empty translations almost always
          // indicate the model gave up, not a parse blip.)
          if (t.text && t.text.trim()) {
            next[t.id] = { status: 'done', text: t.text };
            // Cache by source text so flipping target lang and back
            // re-uses this translation without re-billing the LLM.
            const srcText = idToText.get(t.id);
            if (srcText) langBucket[srcText] = t.text;
          } else {
            next[t.id] = { status: 'error', attempts: next[t.id]?.attempts };
          }
        }
        nextCache[flushLang] = trimCacheBucket(langBucket);
        // Ids we asked about that the LLM didn't return are usually a
        // model-side miss (truncated response, code-fence wrap, dropped
        // entries) — the kind of transient failure that often clears
        // on a fresh batch. Auto-retry up to MAX_AUTO_RETRIES, then
        // give up.
        for (const id of lines.map((l) => l.id)) {
          if (returnedIds.has(id)) continue;
          if (next[id]?.status !== 'pending') continue;
          next[id] = bumpRetry(next[id]);
        }
        return {
          translation: { ...state.translation, byLineId: next, cache: nextCache },
        };
      });
    } catch (e) {
      if (e instanceof LlmDisabledError) {
        set({ translation: { ...get().translation, enabled: false, byLineId: {} } });
        return;
      }
      const post = get();
      if (
        !post.translation.enabled ||
        !post.currentSession ||
        post.translation.targetLang !== flushLang
      ) return;
      const askedIds = new Set(lines.map((l) => l.id));
      set((state) => {
        const next = { ...state.translation.byLineId };
        for (const id of askedIds) {
          if (next[id]?.status === 'pending') next[id] = bumpRetry(next[id]);
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
        byLineId: {
          ...state.translation.byLineId,
          // Reset attempts: the user is taking over from auto-retry,
          // so they get a fresh budget if it fails again.
          [lineId]: { status: 'pending', attempts: 0 },
        },
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

  loadBackendClips: async () => {
    // The always-listening batch pipeline writes processed clips to the
    // `audio_clips` table independently of the live-recording UI. Fetch
    // them here and merge into segments so the Archive view shows clips
    // recorded while the user wasn't on the Live tab.
    try {
      const baseUrl = await getApiBaseUrl();
      const res = await fetch(`${baseUrl}/clips?limit=200`, {
        headers: { 'x-tenant-id': DEV_TENANT_ID },
      });
      if (!res.ok) {
        console.warn('[Actio] loadBackendClips: HTTP', res.status);
        return;
      }
      const data: Array<{
        id: string;
        sessionId: string;
        text: string;
        createdAt: string;
        starred: boolean;
      }> = await res.json();

      // Filter empty clips ("we listened to silence for 5 minutes") so the
      // Archive doesn't fill up with blank cards.
      const withText = data.filter((d) => d.text && d.text.trim().length > 0);

      set((state) => {
        const existing = new Map(state.segments.map((s) => [s.id, s]));
        const merged: Segment[] = withText.map((d) => {
          // Preserve any local starred state for clips we've seen before.
          const prior = existing.get(d.id);
          return {
            id: d.id,
            sessionId: d.sessionId,
            text: d.text,
            createdAt: d.createdAt,
            starred: prior?.starred ?? false,
          };
        });

        // Add any segments that exist locally but aren't in the backend
        // response (e.g. legacy localStorage segments from before the batch
        // pipeline, or clips with starred state we want to keep).
        const backendIds = new Set(merged.map((s) => s.id));
        for (const seg of state.segments) {
          if (!backendIds.has(seg.id)) merged.push(seg);
        }

        merged.sort((a, b) => b.createdAt.localeCompare(a.createdAt));
        const pruned = pruneSegments(merged);
        saveVoiceData(pruned, state.clipInterval);
        return { segments: pruned };
      });
    } catch (e) {
      console.warn('[Actio] loadBackendClips failed:', e);
    }
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
