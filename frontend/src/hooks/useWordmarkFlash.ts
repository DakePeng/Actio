import { useEffect, useState } from 'react';
import type { WordmarkState } from '../components/ActioWordmark';

// One-shot wordmark flashes (e.g. the brief 'success' pulse at the end of a
// dictation paste). The flash takes precedence over the normal state
// derivation in useActioState while it's active, then automatically clears.
//
// Module-level state + a tiny subscriber set keeps every hook instance in
// sync without going through the main zustand store, mirroring how
// useWordmarkPreview already works.

let currentFlash: WordmarkState | null = null;
let flashTimer: number | null = null;
const subscribers = new Set<(v: WordmarkState | null) => void>();

function notify() {
  subscribers.forEach((fn) => fn(currentFlash));
}

export function flashWordmark(state: WordmarkState, durationMs: number) {
  if (flashTimer !== null) {
    window.clearTimeout(flashTimer);
  }
  currentFlash = state;
  notify();
  flashTimer = window.setTimeout(() => {
    currentFlash = null;
    flashTimer = null;
    notify();
  }, durationMs);
}

export function clearWordmarkFlash() {
  if (flashTimer !== null) {
    window.clearTimeout(flashTimer);
    flashTimer = null;
  }
  currentFlash = null;
  notify();
}

export function useWordmarkFlash(): WordmarkState | null {
  const [value, setValue] = useState<WordmarkState | null>(currentFlash);
  useEffect(() => {
    subscribers.add(setValue);
    return () => {
      subscribers.delete(setValue);
    };
  }, []);
  return value;
}
