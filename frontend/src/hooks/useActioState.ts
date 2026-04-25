import { useEffect, useState } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { useWordmarkPreview } from './useWordmarkPreview';
import { useWordmarkFlash } from './useWordmarkFlash';
import type { WordmarkState } from '../components/ActioWordmark';

// Hold durations for transient feedback states. Long enough to register, short
// enough not to linger past the toast that triggered them.
const SUCCESS_HOLD_MS = 1200;
const ERROR_HOLD_MS = 1800;

// Feedback message keys that should flash the error state. Anything not in
// this set falls through to success (if tone === 'success') or is ignored.
// Matching by key rather than introducing a new tone keeps the store API
// backward-compatible.
function isErrorFeedback(messageKey: string | null | undefined) {
  if (!messageKey) return false;
  return /Failed$|\.error\b/i.test(messageKey);
}

// Derives the ActioWordmark state from live app signals. The dictation
// hotkey (Ctrl+Shift+Space) lifecycle drives most of these transitions:
//
//   • dictating (1st press, mic capturing)        → transcribing
//   • dictation finalizing (2nd press, awaiting)  → processing
//   • AI extracting reminders                     → processing
//   • background recording, no dictation          → listening
//   • recent failure toast                        → error   (1.8s)
//   • recent success toast / paste flash          → success (1.2s)
//   • otherwise                                   → standby
export function useActioState(): WordmarkState {
  const isRecording = useVoiceStore((s) => s.isRecording);
  const isExtracting = useStore((s) => s.reminders.some((reminder) => reminder.isExtracting));
  const isDictating = useStore((s) => s.ui.isDictating);
  const isDictationTranscribing = useStore((s) => s.ui.isDictationTranscribing);
  const feedback = useStore((s) => s.ui.feedback);
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);

  const preview = useWordmarkPreview();
  const flash = useWordmarkFlash();
  const [transient, setTransient] = useState<'success' | 'error' | null>(null);

  useEffect(() => {
    if (!feedback) return;
    const message = (feedback as { message?: string }).message ?? null;
    const tone = (feedback as { tone?: string }).tone ?? null;
    if (isErrorFeedback(message)) {
      setTransient('error');
      const id = window.setTimeout(() => setTransient(null), ERROR_HOLD_MS);
      return () => window.clearTimeout(id);
    }
    if (tone === 'success') {
      setTransient('success');
      const id = window.setTimeout(() => setTransient(null), SUCCESS_HOLD_MS);
      return () => window.clearTimeout(id);
    }
  }, [feedback]);

  // Dev preview (Shift+Alt+Tab) takes precedence over every live signal so
  // designers can walk through states without actually recording.
  if (preview) return preview;
  // One-shot flashes triggered by app events (e.g. the success pulse fired
  // when a dictated transcript is pasted) win over the live derivation.
  if (flash) return flash;
  if (transient) return transient;
  if (isDictationTranscribing) return 'processing';
  if (isExtracting) return 'processing';
  if (isDictating) return 'transcribing';
  if (listeningEnabled) return 'listening';
  if (isRecording) return 'listening';
  return 'standby';
}
