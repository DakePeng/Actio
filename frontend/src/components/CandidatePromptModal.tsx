import { useEffect, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useCandidatePrompt } from '../hooks/use-candidate-prompt';
import { useVoiceStore } from '../store/use-voice-store';
import { candidateClipUrl } from '../api/speakers';
import type { VoiceprintCandidate } from '../types/speaker';

const PRESET_COLORS = [
  '#E57373',
  '#F06292',
  '#BA68C8',
  '#64B5F6',
  '#4DB6AC',
  '#81C784',
  '#FFD54F',
  '#FF8A65',
];

function formatMinutes(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  const seconds = Math.floor((ms % 60_000) / 1000);
  if (minutes === 0) return `${seconds}s`;
  if (seconds === 0) return `${minutes}m`;
  return `${minutes}m ${seconds}s`;
}

function CandidateBody({
  candidate,
  onConfirm,
  onDismiss,
  onSnooze,
}: {
  candidate: VoiceprintCandidate;
  onConfirm: (input: { display_name: string; color: string }) => Promise<void>;
  onDismiss: () => Promise<void>;
  onSnooze: () => void;
}) {
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState('');
  const [color, setColor] = useState(PRESET_COLORS[0]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void candidateClipUrl(candidate.audio_ref).then((url) => {
      if (!cancelled) setAudioUrl(url);
    });
    return () => {
      cancelled = true;
    };
  }, [candidate.audio_ref]);

  async function handleConfirm() {
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      await onConfirm({ display_name: trimmed, color });
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
    // On success the hook refreshes and the modal unmounts — no cleanup needed.
  }

  async function handleDismiss() {
    setBusy(true);
    setError(null);
    try {
      await onDismiss();
    } catch (e) {
      setError((e as Error).message);
      setBusy(false);
    }
  }

  return (
    <div className="candidate-modal__card">
      <h2 className="candidate-modal__title">I've been hearing a new voice</h2>
      <p className="candidate-modal__meta">
        {candidate.occurrences}{' '}
        {candidate.occurrences === 1 ? 'time' : 'times'} ·{' '}
        {formatMinutes(candidate.total_duration_ms)} of speech
      </p>
      {audioUrl ? (
        <audio
          className="candidate-modal__audio"
          controls
          src={audioUrl}
          preload="metadata"
        />
      ) : (
        <div className="candidate-modal__audio candidate-modal__audio--loading">
          Loading preview…
        </div>
      )}

      {!naming ? (
        <div className="candidate-modal__actions">
          <button
            type="button"
            className="primary-button"
            onClick={() => setNaming(true)}
            disabled={busy}
          >
            Name this person
          </button>
          <button
            type="button"
            className="secondary-button"
            onClick={() => void handleDismiss()}
            disabled={busy}
          >
            Not a person
          </button>
          <button
            type="button"
            className="secondary-button"
            onClick={onSnooze}
            disabled={busy}
          >
            Ask me later
          </button>
        </div>
      ) : (
        <div className="candidate-modal__form">
          <input
            type="text"
            className="person-form__name-input"
            placeholder="Name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            onKeyDown={(e) => {
              if (e.key === 'Enter') void handleConfirm();
              if (e.key === 'Escape') setNaming(false);
            }}
          />
          <div className="person-form__swatches" role="group" aria-label="Color">
            {PRESET_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                className={`person-form__swatch${color === c ? ' is-selected' : ''}`}
                style={{ backgroundColor: c }}
                onClick={() => setColor(c)}
                aria-label={`Select color ${c}`}
                aria-pressed={color === c}
              />
            ))}
          </div>
          <div className="candidate-modal__actions">
            <button
              type="button"
              className="primary-button"
              onClick={() => void handleConfirm()}
              disabled={busy || !name.trim()}
            >
              {busy ? 'Saving…' : 'Save voiceprint'}
            </button>
            <button
              type="button"
              className="secondary-button"
              onClick={() => setNaming(false)}
              disabled={busy}
            >
              Back
            </button>
          </div>
        </div>
      )}

      {error && <p className="candidate-modal__error">{error}</p>}
    </div>
  );
}

/**
 * Phase C: surfaces a modal when the backend reports a voiceprint candidate
 * that has cleared the evidence bar. Mounted once at the app root.
 */
export function CandidatePromptModal() {
  const { activeCandidate, confirm, dismiss, snooze } = useCandidatePrompt();
  const fetchSpeakers = useVoiceStore((s) => s.fetchSpeakers);

  return (
    <AnimatePresence>
      {activeCandidate && (
        <motion.div
          className="candidate-modal__backdrop"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2 }}
        >
          <motion.div
            className="candidate-modal"
            initial={{ scale: 0.94, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.94, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 300, damping: 26 }}
          >
            <CandidateBody
              candidate={activeCandidate}
              onConfirm={async (input) => {
                await confirm(activeCandidate, input);
                void fetchSpeakers();
              }}
              onDismiss={() => dismiss(activeCandidate)}
              onSnooze={() => snooze(activeCandidate.candidate_id)}
            />
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
