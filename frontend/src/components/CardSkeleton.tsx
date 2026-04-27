import { motion } from 'framer-motion';
import { useT } from '../i18n';

/** Skeleton variant of `<Card>` shown while a freshly-extracted reminder is
 *  still being filled in by the LLM (`reminder.isExtracting === true`).
 *  Mirrors the collapsed-card structure so the layout doesn't jump when the
 *  real content arrives.
 *
 *  Lives in its own component so the Card render path's hook list stays
 *  unconditional — calling `useT()` here and 30+ hooks in `Card` after a
 *  conditional return was a Rules-of-Hooks violation that fired on every
 *  isExtracting → !isExtracting transition (ISSUES.md #85). Two distinct
 *  components → two distinct hook lists → no order corruption. */
export function CardSkeleton() {
  const t = useT();
  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 30 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
    >
      <article
        className="reminder-card card--skeleton"
        aria-busy="true"
        aria-label={t('card.aria.extracting')}
      >
        <div className="reminder-accent" />
        <div className="card-shell">
          <div className="card-head">
            <span className="skeleton-badge" />
            <span className="mini-badge mini-badge--ai skeleton-ai-badge">
              {t('card.aiBadge')}
            </span>
          </div>
          <div className="skeleton-line skeleton-line--title" />
          <div className="skeleton-line skeleton-line--desc" />
          <div className="skeleton-line skeleton-line--desc-short" />
          <div className="card-meta">
            <div className="card-meta__item">
              <span className="skeleton-dot" />
              <span className="skeleton-line skeleton-line--meta" />
            </div>
            <span className="skeleton-line skeleton-line--meta-short" />
          </div>
          <div className="label-row">
            <span className="skeleton-chip" style={{ width: 48 }} />
            <span className="skeleton-chip" style={{ width: 64 }} />
            <span className="skeleton-chip" style={{ width: 40 }} />
          </div>
        </div>
      </article>
    </motion.div>
  );
}
