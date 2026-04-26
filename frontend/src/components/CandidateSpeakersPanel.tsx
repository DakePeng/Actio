import { useCallback, useEffect, useState } from 'react';
import { useT } from '../i18n';
import {
  listCandidateSpeakers,
  promoteCandidateSpeaker,
  dismissCandidateSpeaker,
  type CandidateSpeaker,
} from '../api/speakers';

function formatRelative(iso: string | null, lastHeardUnknown: string): string {
  if (!iso) return lastHeardUnknown;
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return lastHeardUnknown;
  const diffMs = Date.now() - then;
  if (diffMs < 60_000) return 'just now';
  if (diffMs < 3_600_000) return `${Math.floor(diffMs / 60_000)}m ago`;
  if (diffMs < 86_400_000) return `${Math.floor(diffMs / 3_600_000)}h ago`;
  return `${Math.floor(diffMs / 86_400_000)}d ago`;
}

interface RowProps {
  candidate: CandidateSpeaker;
  onPromoted: () => void;
  onDismissed: () => void;
}

function CandidateRow({ candidate, onPromoted, onDismissed }: RowProps) {
  const t = useT();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handlePromote() {
    const trimmed = name.trim();
    setBusy(true);
    setError(null);
    try {
      await promoteCandidateSpeaker(candidate.id, trimmed || undefined);
      onPromoted();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleDismiss() {
    if (!window.confirm(t('candidates.confirmDismiss'))) return;
    setBusy(true);
    setError(null);
    try {
      await dismissCandidateSpeaker(candidate.id);
      onDismissed();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="candidate-row">
      <span
        className="candidate-row__swatch"
        style={{ backgroundColor: candidate.color }}
        aria-hidden
      />
      <div className="candidate-row__body">
        <div className="candidate-row__name">{candidate.display_name}</div>
        <div className="candidate-row__meta">
          {t('candidates.lastHeard', {
            when: formatRelative(
              candidate.last_matched_at,
              t('candidates.lastHeardUnknown'),
            ),
          })}
        </div>
        {error && <div className="candidate-row__error">{error}</div>}
      </div>
      {editing ? (
        <div className="candidate-row__actions">
          <input
            type="text"
            className="candidate-row__input"
            value={name}
            placeholder={t('candidates.namePlaceholder')}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void handlePromote();
              if (e.key === 'Escape') setEditing(false);
            }}
            autoFocus
            disabled={busy}
          />
          <button
            type="button"
            onClick={() => void handlePromote()}
            disabled={busy}
          >
            {t('candidates.save')}
          </button>
          <button
            type="button"
            onClick={() => setEditing(false)}
            disabled={busy}
          >
            {t('candidates.cancel')}
          </button>
        </div>
      ) : (
        <div className="candidate-row__actions">
          <button
            type="button"
            onClick={() => setEditing(true)}
            disabled={busy}
            aria-label={t('candidates.aria.promote', {
              name: candidate.display_name,
            })}
          >
            {t('candidates.promote')}
          </button>
          <button
            type="button"
            onClick={() => void handleDismiss()}
            disabled={busy}
            aria-label={t('candidates.aria.dismiss', {
              name: candidate.display_name,
            })}
          >
            {t('candidates.dismiss')}
          </button>
        </div>
      )}
    </div>
  );
}

export function CandidateSpeakersPanel() {
  const t = useT();
  const [items, setItems] = useState<CandidateSpeaker[]>([]);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const next = await listCandidateSpeakers();
      setItems(next);
    } catch {
      // Backend unavailable is expected in dev — render empty state.
      setItems([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (loading) return null;
  if (items.length === 0) {
    return (
      <section className="candidates-panel">
        <h3 className="candidates-panel__heading">
          {t('candidates.heading')}
        </h3>
        <p className="candidates-panel__subtitle">
          {t('candidates.subtitle')}
        </p>
        <p className="candidates-panel__empty">{t('candidates.empty')}</p>
      </section>
    );
  }

  return (
    <section className="candidates-panel">
      <h3 className="candidates-panel__heading">{t('candidates.heading')}</h3>
      <p className="candidates-panel__subtitle">{t('candidates.subtitle')}</p>
      <ul className="candidates-panel__list">
        {items.map((c) => (
          <li key={c.id}>
            <CandidateRow
              candidate={c}
              onPromoted={refresh}
              onDismissed={refresh}
            />
          </li>
        ))}
      </ul>
    </section>
  );
}
