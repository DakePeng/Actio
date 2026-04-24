import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../store/use-store';
import { useT, type TKey } from '../i18n';

export function FeedbackToast() {
  const feedback = useStore((s) => s.ui.feedback);
  const t = useT();

  return (
    <AnimatePresence>
      {feedback && (
        <motion.div
          className={`feedback-toast feedback-toast--${feedback.tone}`}
          initial={{ opacity: 0, y: 12, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 8, scale: 0.98 }}
          transition={{ duration: 0.2, ease: 'easeOut' }}
        >
          <span className="feedback-toast__dot" aria-hidden="true" />
          <span>{t(feedback.message as TKey, feedback.vars)}</span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
