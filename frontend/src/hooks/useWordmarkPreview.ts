import { useEffect, useState } from 'react';
import type { WordmarkState } from '../components/ActioWordmark';

// Dev/preview shortcut for cycling the ActioWordmark through its states
// without having to actually trigger recording, dictation, or a feedback toast.
//
//   Shift + Alt + Tab  →  transcribing
//   Shift + Alt + Tab  →  processing
//   Shift + Alt + Tab  →  success
//   Shift + Alt + Tab  →  standby
//   Shift + Alt + Tab  →  (clear override — real state resumes)
//   Shift + Alt + Tab  →  transcribing  (loop)
//
// Module-level state + a tiny subscriber set keeps all hook instances in sync
// without touching the main zustand store with dev-only scaffolding.

const CYCLE: (WordmarkState | null)[] = [
  'transcribing',
  'processing',
  'success',
  'standby',
  null,
];

let cycleIndex = -1;
let currentPreview: WordmarkState | null = null;
const subscribers = new Set<(v: WordmarkState | null) => void>();

function notify() {
  subscribers.forEach((fn) => fn(currentPreview));
}

export function advanceWordmarkPreview() {
  cycleIndex = (cycleIndex + 1) % CYCLE.length;
  currentPreview = CYCLE[cycleIndex];
  notify();
}

export function clearWordmarkPreview() {
  cycleIndex = -1;
  currentPreview = null;
  notify();
}

// React-facing hook: returns the current preview (or null) and re-renders
// subscribers whenever it changes.
export function useWordmarkPreview(): WordmarkState | null {
  const [value, setValue] = useState<WordmarkState | null>(currentPreview);
  useEffect(() => {
    subscribers.add(setValue);
    return () => {
      subscribers.delete(setValue);
    };
  }, []);
  return value;
}
