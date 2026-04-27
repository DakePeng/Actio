import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  __pushPendingResolutionForTest,
  __resetPendingResolutionsForTest,
  __pendingResolutionsCountForTest,
  applyPendingResolutions,
} from '../use-voice-store';
import type { TranscriptLine } from '../use-voice-store';

function line(
  id: string,
  start_ms: number,
  end_ms: number,
  overrides: Partial<TranscriptLine> = {},
): TranscriptLine {
  return {
    id,
    text: `line ${id}`,
    start_ms,
    end_ms,
    speaker_id: null,
    resolved: false,
    is_final: true,
    ...overrides,
  };
}

describe('applyPendingResolutions (ISSUES.md #59)', () => {
  beforeEach(() => {
    __resetPendingResolutionsForTest();
  });
  afterEach(() => {
    __resetPendingResolutionsForTest();
  });

  it('returns the input array unchanged when no resolutions are buffered', () => {
    const lines = [line('a', 1_000, 3_000)];
    const out = applyPendingResolutions(lines);
    expect(out).toBe(lines); // referential identity preserved (no allocation)
  });

  it('applies a buffered resolution to a line whose midpoint falls in the resolution window', () => {
    // line midpoint = 2_000; window covers [1_500, 2_500]
    __pushPendingResolutionForTest({
      start_ms: 1_500,
      end_ms: 2_500,
      speaker_id: 'spk-alice',
    });
    const lines = [line('a', 1_000, 3_000)];
    const out = applyPendingResolutions(lines);
    expect(out).not.toBe(lines); // fresh array on change
    expect(out[0]).toMatchObject({
      id: 'a',
      speaker_id: 'spk-alice',
      resolved: true,
    });
    expect(__pendingResolutionsCountForTest()).toBe(0); // consumed
  });

  it('does not clobber lines that are already resolved', () => {
    __pushPendingResolutionForTest({
      start_ms: 1_500,
      end_ms: 2_500,
      speaker_id: 'spk-bob',
    });
    const lines = [
      line('a', 1_000, 3_000, { speaker_id: 'spk-alice', resolved: true }),
    ];
    const out = applyPendingResolutions(lines);
    expect(out).toBe(lines); // no change
    expect(out[0].speaker_id).toBe('spk-alice'); // alice preserved
    expect(__pendingResolutionsCountForTest()).toBe(1); // resolution still buffered
  });

  it('keeps a resolution buffered when no line midpoint falls in its window', () => {
    __pushPendingResolutionForTest({
      start_ms: 5_000,
      end_ms: 6_000,
      speaker_id: 'spk-carol',
    });
    const lines = [line('a', 1_000, 3_000)]; // mid=2_000, far from 5-6k window
    const out = applyPendingResolutions(lines);
    expect(out).toBe(lines);
    expect(__pendingResolutionsCountForTest()).toBe(1); // still buffered
  });

  it('consumes each resolution exactly once across multiple matching lines', () => {
    __pushPendingResolutionForTest({
      start_ms: 1_000,
      end_ms: 5_000,
      speaker_id: 'spk-alice',
    });
    __pushPendingResolutionForTest({
      start_ms: 1_000,
      end_ms: 5_000,
      speaker_id: 'spk-bob',
    });
    // Two lines whose midpoints both fall in [1_000, 5_000].
    const lines = [line('a', 1_000, 3_000), line('b', 3_000, 5_000)];
    const out = applyPendingResolutions(lines);
    // First line claims first resolution; second line claims second.
    expect(out[0].speaker_id).toBe('spk-alice');
    expect(out[0].resolved).toBe(true);
    expect(out[1].speaker_id).toBe('spk-bob');
    expect(out[1].resolved).toBe(true);
    expect(__pendingResolutionsCountForTest()).toBe(0);
  });

  it('drains only consumed resolutions; non-matching ones survive the call', () => {
    __pushPendingResolutionForTest({
      start_ms: 1_500,
      end_ms: 2_500,
      speaker_id: 'spk-alice',
    });
    __pushPendingResolutionForTest({
      start_ms: 9_000,
      end_ms: 10_000,
      speaker_id: 'spk-future',
    });
    const lines = [line('a', 1_000, 3_000)]; // matches first only
    const out = applyPendingResolutions(lines);
    expect(out[0].speaker_id).toBe('spk-alice');
    expect(__pendingResolutionsCountForTest()).toBe(1); // future resolution survives
  });

  it('handles a null speaker_id (the "resolved as Unknown" case)', () => {
    __pushPendingResolutionForTest({
      start_ms: 1_500,
      end_ms: 2_500,
      speaker_id: null,
    });
    const lines = [line('a', 1_000, 3_000)];
    const out = applyPendingResolutions(lines);
    expect(out[0].speaker_id).toBeNull();
    expect(out[0].resolved).toBe(true); // still flipped to resolved
  });

  it('respects single-resolution-per-line: only the first matching resolution applies', () => {
    // Two resolutions both cover line a; only the first should win.
    __pushPendingResolutionForTest({
      start_ms: 1_500,
      end_ms: 2_500,
      speaker_id: 'spk-first',
    });
    __pushPendingResolutionForTest({
      start_ms: 1_500,
      end_ms: 2_500,
      speaker_id: 'spk-second',
    });
    const lines = [line('a', 1_000, 3_000)];
    const out = applyPendingResolutions(lines);
    expect(out[0].speaker_id).toBe('spk-first');
    expect(__pendingResolutionsCountForTest()).toBe(1);
  });
});
