import { AnimatePresence, motion } from 'framer-motion';
import { useStore, pendingReminders } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { useT } from '../i18n';
import { formatTimeShort } from '../utils/time';
import { getLabelById } from '../utils/labels';
import { translateLabelName } from '../i18n/label-names';

/** "Needs review" queue for medium-confidence auto-extracted items.
 *
 *  The window extractor routes `confidence === 'medium'` items here (backend
 *  sets `status='pending'`) instead of straight to the Board, so the user
 *  can sanity-check uncertain guesses. Two actions per card:
 *    * Confirm → reuses `restoreReminder` which PATCHes status='open' and
 *      slides the card onto the Board.
 *    * Dismiss → reuses `archiveReminder` which PATCHes status='archived'.
 *
 *  The card surfaces the LLM's evidence quote prominently so the user can
 *  decide without opening the full Card / trace inspector. */
export function NeedsReviewView() {
  const t = useT();
  const reminders = useStore((s) => s.reminders);
  const labels = useStore((s) => s.labels);
  const speakers = useVoiceStore((s) => s.speakers);
  const restoreReminder = useStore((s) => s.restoreReminder);
  const archiveReminder = useStore((s) => s.archiveReminder);
  const setFeedback = useStore((s) => s.setFeedback);

  const pending = pendingReminders(reminders);

  if (pending.length === 0) {
    return (
      <div className="needs-review__empty">
        <div className="needs-review__empty-title">{t('needsReview.empty.title')}</div>
        <div>{t('needsReview.empty.body')}</div>
      </div>
    );
  }

  const onConfirm = async (id: string) => {
    await restoreReminder(id);
    setFeedback('feedback.reminderConfirmed', 'success');
  };
  const onDismiss = async (id: string) => {
    await archiveReminder(id);
    // Undo affordance — Needs-Review items are uncertain auto-extracted
    // candidates; an accidental Dismiss is easy and the user is unlikely
    // to find their way to the Archive tab to recover (see ISSUES.md #54).
    setFeedback('feedback.reminderDismissed', 'neutral', undefined, {
      labelKey: 'feedback.undo',
      onAction: () => {
        void restoreReminder(id);
      },
    });
  };

  return (
    <div className="needs-review">
      <AnimatePresence initial={false}>
        {pending.map((r) => {
          const speaker = r.speakerId ? speakers.find((s) => s.id === r.speakerId) : null;
          // When the extractor wasn't able to resolve a known speaker it
          // leaves `speakerId` null — fall back to the translated "Unknown"
          // label rather than rendering an empty chip.
          const speakerName = speaker?.display_name ?? null;
          const dueLabel = r.dueTime ? formatTimeShort(r.dueTime) : null;

          return (
            <motion.article
              key={r.id}
              className="needs-review__card"
              layout
              initial={{ opacity: 0, y: 12 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, x: 40, transition: { duration: 0.18 } }}
            >
              <div className="needs-review__title">{r.title || r.description}</div>

              {r.description && r.description !== r.title && (
                <div className="needs-review__desc">{r.description}</div>
              )}

              {r.transcript && (
                <blockquote className="needs-review__excerpt">“{r.transcript}”</blockquote>
              )}

              <div className="needs-review__meta">
                {speakerName && (
                  <span>{t('needsReview.sourceSpeaker', { name: speakerName })}</span>
                )}
                {dueLabel && <span>{dueLabel}</span>}
                {r.labels.slice(0, 3).map((labelId) => {
                  const l = getLabelById(labels, labelId);
                  if (!l) return null;
                  return (
                    <span
                      key={labelId}
                      className="label-chip"
                      style={{
                        background: l.bgColor,
                        color: l.color,
                        borderColor: `${l.color}22`,
                      }}
                    >
                      {translateLabelName(t, l.name)}
                    </span>
                  );
                })}
              </div>

              <div className="needs-review__actions">
                <button
                  type="button"
                  className="secondary-button"
                  aria-label={t('needsReview.dismissAria')}
                  onClick={() => void onDismiss(r.id)}
                >
                  {t('needsReview.dismiss')}
                </button>
                <button
                  type="button"
                  className="primary-button"
                  aria-label={t('needsReview.confirmAria')}
                  onClick={() => void onConfirm(r.id)}
                >
                  {t('needsReview.confirm')}
                </button>
              </div>
            </motion.article>
          );
        })}
      </AnimatePresence>
    </div>
  );
}
