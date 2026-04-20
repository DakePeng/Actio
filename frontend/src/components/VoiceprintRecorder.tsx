import { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
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
// Poll fast while active so the meter feels responsive; slow down otherwise.
const POLL_MS_ACTIVE = 200;
const POLL_MS_IDLE = 700;

// Rough ceiling for normal speech RMS. Normalises the meter bar to 0..1.
const METER_CEILING = 0.25;

const REJECTION_COPY: Record<string, string> = {
  too_short: 'That was too short — try reading the whole line.',
  too_long: 'That was too long — keep each take under 30 seconds.',
  low_quality: 'Audio was too quiet or noisy — try speaking up a bit.',
};

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
      speakerApi.cancelLiveEnrollment(speakerId).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [speakerId]);

  useEffect(() => {
    if (!state) return;
    const interval = state.status === 'active' ? POLL_MS_ACTIVE : POLL_MS_IDLE;
    const tick = async () => {
      try {
        const s = await speakerApi.getLiveEnrollmentStatus();
        if (s) setState(s);
      } catch {
        /* keep polling — transient errors are fine */
      }
    };
    pollTimer.current = window.setInterval(() => void tick(), interval);
    return () => {
      if (pollTimer.current !== null) window.clearInterval(pollTimer.current);
    };
  }, [state]);

  useEffect(() => {
    if (!state || state.status !== 'complete' || closing) return;
    setClosing(true);
    void fetchSpeakers();
    // Longer hold so the success state is clearly readable before the
    // recorder dismisses itself.
    const id = window.setTimeout(onDone, 1800);
    return () => window.clearTimeout(id);
  }, [state, closing, fetchSpeakers, onDone]);

  const captured = state?.captured ?? 0;
  const target = state?.target ?? TARGET;
  const currentPassage = PASSAGES[Math.min(captured, PASSAGES.length - 1)];
  const done = captured >= target;
  const isActive = state?.status === 'active';
  const level = state?.rms_level ?? 0;
  const meterPct = Math.min(1, level / METER_CEILING) * 100;
  // Generous cutoff so even quiet breathing nudges the dot past idle.
  const hearing = isActive && level > 0.005;
  const rejectionHint =
    state?.last_rejected_reason && REJECTION_COPY[state.last_rejected_reason];

  if (done) {
    return (
      <div className="voiceprint-recorder voiceprint-recorder--success">
        <motion.div
          className="voiceprint-recorder__success-check"
          initial={{ scale: 0, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          transition={{ type: 'spring', stiffness: 320, damping: 18 }}
          aria-hidden="true"
        >
          <svg viewBox="0 0 24 24" width="48" height="48" fill="none">
            <motion.circle
              cx="12"
              cy="12"
              r="11"
              fill="#22c55e"
              initial={{ scale: 0.3 }}
              animate={{ scale: 1 }}
              transition={{ type: 'spring', stiffness: 260, damping: 20 }}
            />
            <motion.path
              d="M7.5 12.5l3 3 6-6.5"
              stroke="white"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
              initial={{ pathLength: 0 }}
              animate={{ pathLength: 1 }}
              transition={{ duration: 0.45, ease: 'easeOut', delay: 0.15 }}
            />
          </svg>
        </motion.div>
        <motion.h3
          className="voiceprint-recorder__success-title"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.25, delay: 0.1 }}
        >
          {speakerName} is enrolled!
        </motion.h3>
        <motion.p
          className="voiceprint-recorder__success-sub"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.25, delay: 0.25 }}
        >
          Their voice will now be recognised in transcripts.
        </motion.p>
      </div>
    );
  }

  return (
    <div className="voiceprint-recorder">
      <h3 className="voiceprint-recorder__title">
        Record voiceprint for {speakerName}
      </h3>

      {isActive && !error && (
        <div className="voiceprint-recorder__passage-block">
          <p className="voiceprint-recorder__hint">
            Read this aloud at a normal volume:
          </p>
          <p className="voiceprint-recorder__passage">“{currentPassage}”</p>
        </div>
      )}

      {!state && !error && (
        <p className="voiceprint-recorder__hint">Arming microphone…</p>
      )}

      {isActive && !error && (
        <div className="voiceprint-recorder__meter" aria-label="Microphone input level">
          <div className="voiceprint-recorder__meter-label">
            <motion.span
              className={`voiceprint-recorder__dot${hearing ? ' is-hearing' : ''}`}
              animate={hearing ? { scale: [1, 1.25, 1] } : { scale: 1 }}
              transition={
                hearing
                  ? { duration: 0.9, repeat: Infinity, ease: 'easeInOut' }
                  : { duration: 0.2 }
              }
              aria-hidden="true"
            />
            <span>{hearing ? 'Listening…' : 'Waiting for sound…'}</span>
          </div>
          <div className="voiceprint-recorder__meter-track">
            <motion.div
              className="voiceprint-recorder__meter-fill"
              animate={{ width: `${meterPct}%` }}
              transition={{ type: 'tween', duration: 0.12, ease: 'linear' }}
            />
          </div>
        </div>
      )}

      <AnimatePresence>
        {isActive && rejectionHint && (
          <motion.p
            key={`${state?.version}-rejection`}
            className="voiceprint-recorder__rejection"
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.2 }}
          >
            {rejectionHint}
          </motion.p>
        )}
      </AnimatePresence>

      <div
        className="voiceprint-recorder__captured"
        aria-label={`${captured} of ${target} clips captured`}
      >
        {Array.from({ length: target }).map((_, i) => (
          <motion.span
            key={i}
            className={`voiceprint-recorder__chip${i < captured ? ' is-done' : ''}`}
            animate={
              i === captured
                ? { scale: [1, 1.12, 1], opacity: [0.7, 1, 0.7] }
                : { scale: 1, opacity: 1 }
            }
            transition={
              i === captured
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

      <div className="voiceprint-recorder__actions">
        <button type="button" className="secondary-button" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}
