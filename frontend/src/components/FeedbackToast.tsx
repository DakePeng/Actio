import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../store/use-store';

export function FeedbackToast() {
  const feedback = useStore((s) => s.ui.feedback);

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
          <span>{feedback.message}</span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
