import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import {
  CLOSE_THRESHOLD_PX,
  MOUSE_REVEAL_THRESHOLD_PX,
  TOUCH_REVEAL_THRESHOLD_PX,
} from '../swipe-constants';
import { resolveGestureProfile, useSwipeActionRow } from '../useSwipeActionRow';

describe('resolveGestureProfile', () => {
  it('uses mouse threshold for mouse drags', () => {
    expect(resolveGestureProfile({ pointerType: 'mouse' })).toBe(MOUSE_REVEAL_THRESHOLD_PX);
  });

  it('uses gesture threshold for touch drags', () => {
    expect(resolveGestureProfile({ pointerType: 'touch' })).toBe(TOUCH_REVEAL_THRESHOLD_PX);
  });

  it('uses gesture threshold for horizontal wheel gestures', () => {
    expect(
      resolveGestureProfile({
        interactionType: 'gesture',
        deltaX: 80,
        deltaY: 4,
      }),
    ).toBe(
      TOUCH_REVEAL_THRESHOLD_PX,
    );
  });
});

describe('useSwipeActionRow', () => {
  it('reveals delete after crossing the left threshold', () => {
    const { result } = renderHook(() => useSwipeActionRow());
    act(() => {
      result.current.handleRelease({ x: -MOUSE_REVEAL_THRESHOLD_PX - 1, y: 0, pointerType: 'mouse' });
    });
    expect(result.current.side).toBe('left');
    expect(result.current.phase).toBe('revealed');
  });

  it('returns to idle inside the close threshold', () => {
    const { result } = renderHook(() => useSwipeActionRow());
    act(() => {
      result.current.handleRelease({ x: CLOSE_THRESHOLD_PX - 1, y: 0, pointerType: 'mouse' });
    });
    expect(result.current.phase).toBe('idle');
  });

  it('keeps horizontal mouse drags below the mouse threshold closed', () => {
    const { result } = renderHook(() => useSwipeActionRow());
    act(() => {
      result.current.handleRelease({ x: 60, y: 0, pointerType: 'mouse' });
    });
    expect(result.current.side).toBe(null);
    expect(result.current.phase).toBe('idle');
  });

  it('requires two taps to execute a revealed action', () => {
    const { result } = renderHook(() => useSwipeActionRow());
    act(() => {
      result.current.reveal('right');
      result.current.confirmAction('right');
    });
    expect(result.current.phase).toBe('confirm');
    act(() => {
      result.current.confirmAction('right');
    });
    expect(result.current.phase).toBe('executing');
  });
});
