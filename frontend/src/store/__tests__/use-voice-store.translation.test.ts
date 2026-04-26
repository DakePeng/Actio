import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useVoiceStore } from '../use-voice-store';
import type { TranscriptLine } from '../use-voice-store';
import * as translateApi from '../../api/translate';

function mkLine(id: string, text: string, isFinal = true): TranscriptLine {
  return {
    id,
    text,
    start_ms: 0,
    end_ms: 0,
    speaker_id: null,
    resolved: true,
    is_final: isFinal,
  };
}

beforeEach(() => {
  // Use Chinese text so the same-script-skip optimisation doesn't
  // short-circuit translation when target is 'en'. Tests that
  // specifically exercise the skip path use Latin text explicitly.
  useVoiceStore.setState({
    isRecording: true,
    currentSession: {
      id: 'live',
      startedAt: new Date().toISOString(),
      lines: [mkLine('a', '你好世界'), mkLine('b', '今天天气真好'), mkLine('c', '部分文字', false)],
      pendingPartial: null,
      pipelineReady: true,
    },
    translation: {
      enabled: false,
      targetLang: 'en',
      byLineId: {},
      cache: {},
    },
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('translation slice', () => {
  it('setTranslationEnabled(true) backfills only finalized lines', async () => {
    const spy = vi
      .spyOn(translateApi, 'translateLines')
      .mockResolvedValue([
        { id: 'a', text: 'Hello world' },
        { id: 'b', text: 'The weather is nice today' },
      ]);
    await useVoiceStore.getState().setTranslationEnabled(true);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]?.[1].map((l) => l.id)).toEqual(['a', 'b']);
    const t = useVoiceStore.getState().translation;
    expect(t.enabled).toBe(true);
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'Hello world' });
    expect(t.byLineId['b']).toEqual({ status: 'done', text: 'The weather is nice today' });
    expect(t.byLineId['c']).toBeUndefined();
  });

  it('setTranslationTargetLang clears byLineId; same-script lines short-circuit', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'done', text: 'Hello world' },
        },
        cache: {},
      },
    });
    const spy = vi.spyOn(translateApi, 'translateLines').mockResolvedValue([]);
    await useVoiceStore.getState().setTranslationTargetLang('zh-CN');
    const t = useVoiceStore.getState().translation;
    expect(t.targetLang).toBe('zh-CN');
    // Lines a + b are Chinese → already match zh-CN target → no LLM call.
    expect(spy).not.toHaveBeenCalled();
    expect(t.byLineId['a']).toEqual({ status: 'done', text: '你好世界' });
    expect(t.byLineId['b']).toEqual({ status: 'done', text: '今天天气真好' });
  });

  it('queueLineForTranslation only adds when enabled', () => {
    useVoiceStore.getState().queueLineForTranslation('a');
    expect(useVoiceStore.getState().translation.byLineId['a']).toBeUndefined();

    useVoiceStore.setState({
      translation: { enabled: true, targetLang: 'en', byLineId: {}, cache: {} },
    });
    // Line 'a' is Chinese, target is 'en' → still gets queued as pending.
    useVoiceStore.getState().queueLineForTranslation('a');
    expect(useVoiceStore.getState().translation.byLineId['a']).toEqual({ status: 'pending' });
  });

  it('flushTranslationBatch sends all pending and marks done on success', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'pending' },
          b: { status: 'pending' },
          c: { status: 'done', text: 'cached' },
        },
        cache: {},
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockResolvedValue([
      { id: 'a', text: 'A!' },
      { id: 'b', text: 'B!' },
    ]);
    await useVoiceStore.getState().flushTranslationBatch();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'A!' });
    expect(t.byLineId['b']).toEqual({ status: 'done', text: 'B!' });
    expect(t.byLineId['c']).toEqual({ status: 'done', text: 'cached' });
  });

  it('auto-retries after a rejected batch; only marks error after exceeding the retry budget', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
        cache: {},
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockRejectedValue(new Error('boom'));

    // 1st attempt: bumps to pending+attempts=1.
    await useVoiceStore.getState().flushTranslationBatch();
    expect(useVoiceStore.getState().translation.byLineId['a']).toEqual({
      status: 'pending',
      attempts: 1,
    });

    // 2nd attempt: pending+attempts=2 (still inside budget).
    await useVoiceStore.getState().flushTranslationBatch();
    expect(useVoiceStore.getState().translation.byLineId['a']).toEqual({
      status: 'pending',
      attempts: 2,
    });

    // 3rd attempt: exceeds MAX_AUTO_RETRIES, becomes user-facing error.
    await useVoiceStore.getState().flushTranslationBatch();
    expect(useVoiceStore.getState().translation.byLineId['a']).toEqual({
      status: 'error',
      attempts: 3,
    });
  });

  it('manual retry resets the attempts counter', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'error', attempts: 3 } },
        cache: {},
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockRejectedValue(new Error('boom'));
    useVoiceStore.getState().retryTranslationLine('a');
    // retryTranslationLine kicks off a flush; await microtask for it.
    await new Promise((r) => setTimeout(r, 0));
    // After the manual retry's flush failed once, attempts should be 1
    // (reset from 3, then bumped) — not 4.
    expect(useVoiceStore.getState().translation.byLineId['a']).toEqual({
      status: 'pending',
      attempts: 1,
    });
  });

  it('LlmDisabledError flips enabled off and clears pending', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
        cache: {},
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockRejectedValue(new translateApi.LlmDisabledError());
    await useVoiceStore.getState().flushTranslationBatch();
    const t = useVoiceStore.getState().translation;
    expect(t.enabled).toBe(false);
    expect(t.byLineId).toEqual({});
  });

  it('stopRecording clears byLineId but keeps targetLang', () => {
    useVoiceStore.setState({
      _ws: null,
      translation: {
        enabled: true,
        targetLang: 'ja',
        byLineId: { a: { status: 'done', text: 'A!' } },
        cache: {},
      },
    });
    useVoiceStore.getState().stopRecording();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId).toEqual({});
    expect(t.targetLang).toBe('ja');
  });

  it('auto-retries ids missing from the LLM response; marks error only after the budget is spent', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' }, b: { status: 'pending' } },
        cache: {},
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockResolvedValue([{ id: 'a', text: 'A!' }]);
    await useVoiceStore.getState().flushTranslationBatch();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'A!' });
    // 'b' was dropped by the LLM — re-pended for next tick (attempt 1).
    expect(t.byLineId['b']).toEqual({ status: 'pending', attempts: 1 });
  });

  it('drops the response if the user mutes mid-flush', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
        cache: {},
      },
    });
    let resolve: (v: { id: string; text: string }[]) => void = () => {};
    vi.spyOn(translateApi, 'translateLines').mockImplementation(
      () => new Promise((r) => { resolve = r; }),
    );
    const flush = useVoiceStore.getState().flushTranslationBatch();
    // Simulate stopRecording mid-flush: clears byLineId.
    useVoiceStore.setState({
      currentSession: null,
      translation: { ...useVoiceStore.getState().translation, byLineId: {} },
    });
    resolve([{ id: 'a', text: 'A!' }]);
    await flush;
    expect(useVoiceStore.getState().translation.byLineId).toEqual({});
  });

  it('caps each batch at 4 lines; remainder waits for next tick', async () => {
    useVoiceStore.setState({
      currentSession: {
        id: 'live',
        startedAt: '',
        lines: [
          mkLine('a', 'one'),
          mkLine('b', 'two'),
          mkLine('c', 'three'),
          mkLine('d', 'four'),
          mkLine('e', 'five'),
          mkLine('f', 'six'),
        ],
        pendingPartial: null,
        pipelineReady: true,
      },
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'pending' },
          b: { status: 'pending' },
          c: { status: 'pending' },
          d: { status: 'pending' },
          e: { status: 'pending' },
          f: { status: 'pending' },
        },
        cache: {},
      },
    });
    const spy = vi.spyOn(translateApi, 'translateLines').mockResolvedValue([]);
    await useVoiceStore.getState().flushTranslationBatch();
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]?.[1]).toHaveLength(4);
    expect(spy.mock.calls[0]?.[1].map((l) => l.id)).toEqual(['a', 'b', 'c', 'd']);
    // 'e' and 'f' stay pending for the next tick.
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId['e']?.status).toBe('pending');
    expect(t.byLineId['f']?.status).toBe('pending');
  });

  it('marks empty/whitespace-only LLM responses as error, not silent done', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' }, b: { status: 'pending' } },
        cache: {},
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockResolvedValue([
      { id: 'a', text: '' },
      { id: 'b', text: '   ' },
    ]);
    await useVoiceStore.getState().flushTranslationBatch();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId['a']?.status).toBe('error');
    expect(t.byLineId['b']?.status).toBe('error');
  });

  it('drops the response if targetLang changes mid-flush', async () => {
    // Without the lang-snapshot guard, an in-flight English flush would
    // overwrite zh-CN's freshly-cleared byLineId with the EN translations,
    // and the user would see English subtitles labeled as Chinese.
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
        cache: {},
      },
    });
    let resolve: (v: { id: string; text: string }[]) => void = () => {};
    vi.spyOn(translateApi, 'translateLines').mockImplementation(
      () => new Promise((r) => { resolve = r; }),
    );
    const flush = useVoiceStore.getState().flushTranslationBatch();
    // Mid-flush, user picks a different target language. The store would
    // normally call setTranslationTargetLang here; we simulate just the
    // state mutation that matters.
    useVoiceStore.setState({
      translation: {
        ...useVoiceStore.getState().translation,
        targetLang: 'zh-CN',
        byLineId: { a: { status: 'pending' } },
        cache: {},
      },
    });
    resolve([{ id: 'a', text: 'HELLO_EN' }]);
    await flush;
    // The EN response must NOT have landed on the zh-CN-labeled entry.
    expect(useVoiceStore.getState().translation.byLineId['a']).toEqual({
      status: 'pending',
    });
  });

  it('queueLineForTranslation skips LLM when source script matches target', () => {
    useVoiceStore.setState({
      currentSession: {
        id: 'live',
        startedAt: '',
        lines: [mkLine('en1', 'Hello world')],
        pendingPartial: null,
        pipelineReady: true,
      },
      translation: { enabled: true, targetLang: 'en', byLineId: {}, cache: {} },
    });
    useVoiceStore.getState().queueLineForTranslation('en1');
    // Source already English → marked done with text === source so the
    // UI's passthrough-suppression renders no annotation. No LLM round-trip.
    expect(useVoiceStore.getState().translation.byLineId['en1']).toEqual({
      status: 'done',
      text: 'Hello world',
    });
  });

  it('cache hit returns prior translation instantly on lang-flip', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' }, b: { status: 'pending' } },
        cache: {},
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockResolvedValue([
      { id: 'a', text: 'Hello world' },
      { id: 'b', text: 'The weather is nice today' },
    ]);
    await useVoiceStore.getState().flushTranslationBatch();
    // Cache should now hold both source→translation pairs.
    const cache = useVoiceStore.getState().translation.cache;
    expect(cache['en']?.['你好世界']).toBe('Hello world');
    expect(cache['en']?.['今天天气真好']).toBe('The weather is nice today');

    // Flip to ja (no cache yet) then back to en — cached entries
    // should re-appear without another LLM call.
    const spy = vi.spyOn(translateApi, 'translateLines').mockClear();
    await useVoiceStore.getState().setTranslationTargetLang('ja');
    spy.mockClear();
    await useVoiceStore.getState().setTranslationTargetLang('en');
    expect(spy).not.toHaveBeenCalled();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'Hello world' });
    expect(t.byLineId['b']).toEqual({ status: 'done', text: 'The weather is nice today' });
  });

  it('cache bucket is capped at MAX_CACHE_ENTRIES_PER_LANG; oldest entries are evicted', async () => {
    // Pre-fill the cache with 199 fake entries so a new translation
    // brings the bucket up to the cap, and the next one (200th + 1)
    // forces eviction.
    const seed: Record<string, string> = {};
    for (let i = 0; i < 199; i++) seed[`src-${i}`] = `tx-${i}`;
    useVoiceStore.setState({
      currentSession: {
        id: 'live',
        startedAt: '',
        lines: [
          mkLine('a', '一句新的中文'),
          mkLine('b', '另一句新的中文'),
        ],
        pendingPartial: null,
        pipelineReady: true,
      },
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' }, b: { status: 'pending' } },
        cache: { en: seed },
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockResolvedValue([
      { id: 'a', text: 'A new sentence' },
      { id: 'b', text: 'Another new sentence' },
    ]);
    await useVoiceStore.getState().flushTranslationBatch();
    const cache = useVoiceStore.getState().translation.cache;
    const entries = Object.keys(cache['en']!);
    // 199 seeded + 2 new = 201; cap is 200; oldest 1 evicted.
    expect(entries.length).toBe(200);
    // The two newest source texts must still be present.
    expect(cache['en']!['一句新的中文']).toBe('A new sentence');
    expect(cache['en']!['另一句新的中文']).toBe('Another new sentence');
    // The oldest seeded entry (insertion order) is gone.
    expect(cache['en']!['src-0']).toBeUndefined();
  });
});
