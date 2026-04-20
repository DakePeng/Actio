import { useEffect, useRef, useState } from 'react';
import { motion } from 'framer-motion';
import * as speakerApi from '../api/speakers';
import { useVoiceStore } from '../store/use-voice-store';
import type { LiveEnrollmentState } from '../types/speaker';

// Longer, more varied passages give the embedding model a better chance to
// capture a speaker's prosody. Five clips × ~5 seconds each lands around
// the 25 s total that the 3D-Speaker family typically recommends.
const PASSAGES = [
  'The quick brown fox jumps over the lazy dog, and then sits down for a long rest.',
  'She sells seashells by the seashore under a clear blue sky on a warm summer afternoon.',
  'A journey of a thousand miles begins with a single step, though most journeys are rarely that simple.',
  'Peter Piper picked a peck of pickled peppers, and the whole kitchen smelled like vinegar for days.',
  'How much wood would a woodchuck chuck if a woodchuck could chuck wood all afternoon?',
];

const TARGET = 5;
const POLL_MS = 700;

/**
 * Drives live voiceprint enrollment: arms the backend audio pipeline, then
 * polls status until the target count is reached. No getUserMedia — the
 * Rust backend is already capturing audio via cpal, we just tell it to
 * save the next few quality-passing utterances as this speaker's voiceprints.
 */
export function VoiceprintRecorder({
  speakerId,
  speakerName,
  onDone,
  onCancel,
}: {
  speakerId: string;
  speakerName: string;
  onDone: () => void;
  onCancel: () => void;
}) {
  const [state, setState] = useState<LiveEnrollmentState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [closing, setClosing] = useState(false);
  const pollTimer = useRef<number | null>(null);
  const fetchSpeakers = useVoiceStore((s) => s.fetchSpeakers);

  // Start enrollment on mount; cancel on unmount if we didn't finish.
  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const s = await speakerApi.startLiveEnrollment(speakerId, TARGET);
        if (!mounted) return;
        setState(s);
      } catch (e) {
        if (!mounted) return;
        setError((e as Error).message);
      }
    })();
    return () => {
      mounted = false;
      // Fire-and-forget cancel — if the user closed mid-way, don't leave
      // enrollment armed for the next unrelated utterance.
      speakerApi.cancelLiveEnrollment(speakerId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [speakerId]);

  // Poll status while active.
  useEffect(() => {
    if (!state || state.status !== 'active') return;
    const tick = async () => {
      try {
        const s = await speakerApi.getLiveEnrollmentStatus();
        setState(s);
      } catch {
        /* keep polling — transient errors are fine */
      }
    };
    pollTimer.current = window.setInterval(() => void tick(), POLL_MS);
    return () => {
      if (pollTimer.current !== null) window.clearInterval(pollTimer.current);
    };
  }, [state]);

  // When the backend reports complete, refresh the speaker list and close.
  useEffect(() => {
    if (!state || state.status !== 'complete' || closing) return;
    setClosing(true);
    void fetchSpeakers();
    // Give the "3/3 captured" state a beat to render before dismissing.
    const id = window.setTimeout(onDone, 900);
    return () => window.clearTimeout(id);
  }, [state, closing, fetchSpeakers, onDone]);

  const captured = state?.captured ?? 0;
  const target = state?.target ?? TARGET;
  const currentPassage = PASSAGES[Math.min(captured, PASSAGES.length - 1)];
  const done = captured >= target;

  return (
    <div className="voiceprint-recorder">
      <h3 className="voiceprint-recorder__title">
        {done
          ? `Got it — voiceprint saved for ${speakerName}`
          : `Record voiceprint for ${speakerName}`}
      </h3>
      {!done && state?.status === 'active' && (
        <>
          <p className="voiceprint-recorder__hint">
            Read this aloud at a normal volume:
          </p>
          <p className="voiceprint-recorder__passage">“{currentPassage}”</p>
        </>
      )}
      {!state && !error && (
        <p className="voiceprint-recorder__hint">Arming microphone…</p>
      )}

      <div
        className="voiceprint-recorder__captured"
        aria-label={`${captured} of ${target} clips captured`}
      >
        {Array.from({ length: target }).map((_, i) => (
          <motion.span
            key={i}
            className={`voiceprint-recorder__chip${i < captured ? ' is-done' : ''}`}
            animate={
              i === captured && !done
                ? { scale: [1, 1.12, 1], opacity: [0.7, 1, 0.7] }
                : { scale: 1, opacity: 1 }
            }
            transition={
              i === captured && !done
                ? { duration: 1.3, repeat: Infinity, ease: 'easeInOut' }
                : { duration: 0.2 }
            }
          >
            {i < captured ? '✓' : '·'}
          </motion.span>
        ))}
      </div>

      {error && <p className="voiceprint-recorder__error">{error}</p>}
      {state?.status === 'cancelled' && (
        <p className="voiceprint-recorder__error">Enrollment cancelled.</p>
      )}

      {!done && (
        <div className="voiceprint-recorder__actions">
          <button type="button" className="secondary-button" onClick={onCancel}>
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}
