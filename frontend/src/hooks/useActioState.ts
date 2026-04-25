import { useEffect, useState } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { useWordmarkPreview } from './useWordmarkPreview';
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

// Derives the ActioWordmark state from live app signals:
//   • recording or dictating → listening
//   • dictation transcribing → processing
//   • recent success toast   → success (1.2s)
//   • recent failure toast   → error   (1.8s)
//   • otherwise              → standby
export function useActioState(): WordmarkState {
  const isRecording = useVoiceStore((s) => s.isRecording);
  const isExtracting = useStore((s) => s.reminders.some((reminder) => reminder.isExtracting));
  const isDictating = useStore((s) => s.ui.isDictating);
  const isDictationTranscribing = useStore((s) => s.ui.isDictationTranscribing);
  const feedback = useStore((s) => s.ui.feedback);

  const preview = useWordmarkPreview();
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
  if (transient) return transient;
  if (isDictationTranscribing) return 'transcribing';
  if (isExtracting) return 'processing';
  if (isDictating || isRecording) return 'listening';
  return 'standby';
}
