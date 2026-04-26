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
  useVoiceStore.setState({
    isRecording: true,
    currentSession: {
      id: 'live',
      startedAt: new Date().toISOString(),
      lines: [mkLine('a', 'hello'), mkLine('b', 'world'), mkLine('c', 'partial', false)],
      pendingPartial: null,
      pipelineReady: true,
    },
    translation: {
      enabled: false,
      targetLang: 'en',
      byLineId: {},
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
        { id: 'a', text: 'HELLO' },
        { id: 'b', text: 'WORLD' },
      ]);
    await useVoiceStore.getState().setTranslationEnabled(true);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy.mock.calls[0]?.[1].map((l) => l.id)).toEqual(['a', 'b']);
    const t = useVoiceStore.getState().translation;
    expect(t.enabled).toBe(true);
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'HELLO' });
    expect(t.byLineId['b']).toEqual({ status: 'done', text: 'WORLD' });
    expect(t.byLineId['c']).toBeUndefined();
  });

  it('setTranslationTargetLang clears byLineId and re-batches', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: {
          a: { status: 'done', text: 'HELLO' },
        },
      },
    });
    const spy = vi.spyOn(translateApi, 'translateLines').mockResolvedValue([]);
    await useVoiceStore.getState().setTranslationTargetLang('zh-CN');
    const t = useVoiceStore.getState().translation;
    expect(t.targetLang).toBe('zh-CN');
    expect(spy).toHaveBeenCalledWith('zh-CN', expect.arrayContaining([
      expect.objectContaining({ id: 'a' }),
      expect.objectContaining({ id: 'b' }),
    ]));
  });

  it('queueLineForTranslation only adds when enabled', () => {
    useVoiceStore.getState().queueLineForTranslation('new-id');
    expect(useVoiceStore.getState().translation.byLineId['new-id']).toBeUndefined();

    useVoiceStore.setState({
      translation: { enabled: true, targetLang: 'en', byLineId: {} },
    });
    useVoiceStore.getState().queueLineForTranslation('new-id');
    expect(useVoiceStore.getState().translation.byLineId['new-id']).toEqual({ status: 'pending' });
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

  it('flushTranslationBatch marks pending as error on rejection', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockRejectedValue(new Error('boom'));
    await useVoiceStore.getState().flushTranslationBatch();
    expect(useVoiceStore.getState().translation.byLineId['a']?.status).toBe('error');
  });

  it('LlmDisabledError flips enabled off and clears pending', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
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
      },
    });
    useVoiceStore.getState().stopRecording();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId).toEqual({});
    expect(t.targetLang).toBe('ja');
  });

  it('marks ids missing from the LLM response as error instead of leaving them pending', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' }, b: { status: 'pending' } },
      },
    });
    vi.spyOn(translateApi, 'translateLines').mockResolvedValue([{ id: 'a', text: 'A!' }]);
    await useVoiceStore.getState().flushTranslationBatch();
    const t = useVoiceStore.getState().translation;
    expect(t.byLineId['a']).toEqual({ status: 'done', text: 'A!' });
    expect(t.byLineId['b']?.status).toBe('error');
  });

  it('drops the response if the user mutes mid-flush', async () => {
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
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

  it('drops the response if targetLang changes mid-flush', async () => {
    // Without the lang-snapshot guard, an in-flight English flush would
    // overwrite zh-CN's freshly-cleared byLineId with the EN translations,
    // and the user would see English subtitles labeled as Chinese.
    useVoiceStore.setState({
      translation: {
        enabled: true,
        targetLang: 'en',
        byLineId: { a: { status: 'pending' } },
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
      },
    });
    resolve([{ id: 'a', text: 'HELLO_EN' }]);
    await flush;
    // The EN response must NOT have landed on the zh-CN-labeled entry.
    expect(useVoiceStore.getState().translation.byLineId['a']).toEqual({
      status: 'pending',
    });
  });
});
