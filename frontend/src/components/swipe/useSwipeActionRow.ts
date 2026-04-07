import { useState } from 'react';
import {
  CLOSE_THRESHOLD_PX,
  HORIZONTAL_INTENT_RATIO,
  MOUSE_REVEAL_THRESHOLD_PX,
  TOUCH_REVEAL_THRESHOLD_PX,
} from './swipe-constants';

export type SwipeSide = 'left' | 'right' | null;
export type SwipePhase = 'idle' | 'revealed' | 'confirm' | 'executing';

export function resolveGestureProfile(input: {
  interactionType?: 'pointer' | 'gesture';
  pointerType?: string;
  deltaX?: number;
  deltaY?: number;
}) {
  if (input.interactionType === 'gesture') {
    const deltaX = Math.abs(input.deltaX ?? 0);
    const deltaY = Math.abs(input.deltaY ?? 0);
    if (deltaX > Math.max(deltaY, 1) * HORIZONTAL_INTENT_RATIO) {
      return TOUCH_REVEAL_THRESHOLD_PX;
    }

    return MOUSE_REVEAL_THRESHOLD_PX;
  }

  return input.pointerType === 'touch' || input.pointerType === 'pen'
    ? TOUCH_REVEAL_THRESHOLD_PX
    : MOUSE_REVEAL_THRESHOLD_PX;
}

export function useSwipeActionRow() {
  const [state, setState] = useState<{ side: SwipeSide; phase: SwipePhase }>({
    side: null,
    phase: 'idle',
  });

  const reveal = (nextSide: Exclude<SwipeSide, null>) => {
    setState({ side: nextSide, phase: 'revealed' });
  };

  const close = () => {
    setState({ side: null, phase: 'idle' });
  };

  const handleRelease = ({ x, y, pointerType }: { x: number; y: number; pointerType?: string }) => {
    if (Math.abs(x) <= CLOSE_THRESHOLD_PX) {
      close();
      return;
    }

    const threshold = resolveGestureProfile({
      interactionType: 'pointer',
      pointerType,
      deltaX: x,
      deltaY: y,
    });
    if (Math.abs(x) < threshold) {
      close();
      return;
    }

    reveal(x < 0 ? 'left' : 'right');
  };

  const confirmAction = (target: Exclude<SwipeSide, null>) => {
    setState((current) => {
      if (current.side !== target || current.phase === 'idle') {
        return { side: target, phase: 'revealed' };
      }

      if (current.phase === 'revealed') {
        return { ...current, phase: 'confirm' };
      }

      if (current.phase === 'confirm') {
        return { ...current, phase: 'executing' };
      }

      return current;
    });
  };

  return { side: state.side, phase: state.phase, reveal, close, handleRelease, confirmAction };
}
