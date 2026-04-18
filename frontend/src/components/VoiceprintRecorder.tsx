import { useCallback, useEffect, useState } from 'react';
import { useMediaRecorder } from '../hooks/use-media-recorder';
import { useVoiceStore } from '../store/use-voice-store';

const PASSAGES = [
  'The quick brown fox jumps over the lazy dog.',
  'She sells seashells by the seashore under a clear blue sky.',
  'A journey of a thousand miles begins with a single step.',
];

const MAX_CLIP_SEC = 20;
const MIN_CLIP_SEC = 3;

export function VoiceprintRecorder({
  speakerId,
  onDone,
  onCancel,
}: {
  speakerId: string;
  onDone: (warnings: string[]) => void;
  onCancel: () => void;
}) {
  const enroll = useVoiceStore((s) => s.enrollSpeaker);
  const [clips, setClips] = useState<Blob[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const rec = useMediaRecorder();

  const idx = clips.length;
  const done = idx >= 3;

  const toggle = useCallback(async () => {
    if (!rec.recording) {
      try {
        await rec.start();
      } catch {
        /* error surfaced via rec.error */
      }
      return;
    }
    try {
      const blob = await rec.stop();
      setClips((cs) => [...cs, blob]);
    } catch {
      /* ignore — cleanup already ran inside stop */
    }
  }, [rec]);

  // Auto-stop at the hard cap so the server never rejects for duration > 30s.
  useEffect(() => {
    if (rec.recording && rec.durationSec >= MAX_CLIP_SEC) {
      void toggle();
    }
  }, [rec.recording, rec.durationSec, toggle]);

  async function finish() {
    if (clips.length === 0) return;
    setSubmitting(true);
    setSubmitError(null);
    try {
      const result = await enroll(speakerId, clips);
      onDone(result.warnings);
    } catch (err) {
      setSubmitError((err as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  const currentDurationTooShort =
    rec.recording && rec.durationSec > 0 && rec.durationSec < MIN_CLIP_SEC;

  return (
    <div className="voiceprint-recorder">
      <h3>{done ? 'Review' : `Record voiceprint — step ${idx + 1} of 3`}</h3>
      {!done && <p className="voiceprint-recorder__passage">“{PASSAGES[idx]}”</p>}
      {!done && (
        <div className="voiceprint-recorder__meter" aria-hidden="true">
          <div
            className="voiceprint-recorder__bar"
            style={{ width: `${Math.min(100, rec.rmsLevel * 500)}%` }}
          />
        </div>
      )}
      {!done && (
        <div className="voiceprint-recorder__timer">
          {rec.durationSec.toFixed(1)}s / {MAX_CLIP_SEC}s
          {currentDurationTooShort && ' — keep going past 3s'}
        </div>
      )}
      <div className="voiceprint-recorder__captured">
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className={`voiceprint-recorder__chip${i < clips.length ? ' is-done' : ''}`}
            aria-label={i < clips.length ? 'clip captured' : 'clip pending'}
          >
            {i < clips.length ? '✓' : '·'}
          </span>
        ))}
      </div>
      {rec.error && <p className="voiceprint-recorder__error">{rec.error}</p>}
      {submitError && <p className="voiceprint-recorder__error">{submitError}</p>}
      <div className="voiceprint-recorder__actions">
        {!done && (
          <button type="button" className="primary-button" onClick={() => void toggle()}>
            {rec.recording ? '■ Stop' : '● Record'}
          </button>
        )}
        {clips.length > 0 && !rec.recording && (
          <button
            type="button"
            className="primary-button"
            disabled={submitting}
            onClick={() => void finish()}
          >
            {submitting
              ? 'Saving…'
              : done
                ? 'Save voiceprint'
                : `Save (${clips.length})`}
          </button>
        )}
        <button
          type="button"
          className="secondary-button"
          onClick={onCancel}
          disabled={submitting}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
