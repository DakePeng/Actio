import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
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

function formatDuration(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  const seconds = Math.floor((ms % 60_000) / 1000);
  if (minutes === 0) return `${seconds}s`;
  if (seconds === 0) return `${minutes}m`;
  return `${minutes}m ${seconds}s`;
}

function CandidateRow({
  candidate,
  onConfirm,
  onDismiss,
}: {
  candidate: VoiceprintCandidate;
  onConfirm: (input: { display_name: string; color: string }) => Promise<void>;
  onDismiss: () => Promise<void>;
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
    <motion.li
      className="pending-voice"
      layout
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, x: -16, transition: { duration: 0.15 } }}
    >
      <div className="pending-voice__header">
        <div className="pending-voice__avatar" aria-hidden="true">?</div>
        <div className="pending-voice__meta">
          <span className="pending-voice__title">New voice</span>
          <span className="pending-voice__stats">
            heard {candidate.occurrences}{' '}
            {candidate.occurrences === 1 ? 'time' : 'times'} ·{' '}
            {formatDuration(candidate.total_duration_ms)}
          </span>
        </div>
      </div>

      {audioUrl ? (
        <audio
          className="pending-voice__audio"
          controls
          src={audioUrl}
          preload="metadata"
        />
      ) : (
        <div className="pending-voice__audio pending-voice__audio--loading">
          Loading preview…
        </div>
      )}

      {!naming ? (
        <div className="pending-voice__actions">
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
        </div>
      ) : (
        <div className="pending-voice__form">
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
          <div className="pending-voice__actions">
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

      {error && <p className="pending-voice__error">{error}</p>}
    </motion.li>
  );
}

/**
 * Pending voiceprint-candidates — shown as a collapsible section in the
 * People tab. Replaces the earlier auto-popping modal so the user can
 * deal with candidates on their own time.
 */
export function PendingVoicesPanel() {
  const { candidates, confirm, dismiss } = useCandidatePrompt();
  const fetchSpeakers = useVoiceStore((s) => s.fetchSpeakers);

  if (candidates.length === 0) return null;

  return (
    <details className="pending-voices" open>
      <summary>
        New voices to identify ({candidates.length})
      </summary>
      <ul className="pending-voices__list">
        <AnimatePresence initial={false}>
          {candidates.map((c) => (
            <CandidateRow
              key={c.candidate_id}
              candidate={c}
              onConfirm={async (input) => {
                await confirm(c, input);
                void fetchSpeakers();
              }}
              onDismiss={() => dismiss(c)}
            />
          ))}
        </AnimatePresence>
      </ul>
    </details>
  );
}
