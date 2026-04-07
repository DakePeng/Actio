import { useEffect, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';

export function OnboardingCard() {
  const setHasSeenOnboarding = useStore((s) => s.setHasSeenOnboarding);
  const [visible, setVisible] = useState(true);
  const [progressWidth, setProgressWidth] = useState(100);

  useEffect(() => {
    const timer = setTimeout(() => {
      setVisible(false);
      setProgressWidth(0);
      setTimeout(() => setHasSeenOnboarding(true), 500);
    }, 5000);

    const interval = setInterval(() => {
      setProgressWidth((prev) => Math.max(0, prev - 2));
    }, 100);

    return () => {
      clearTimeout(timer);
      clearInterval(interval);
    };
  }, [setHasSeenOnboarding]);

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 0, y: 40, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 20, scale: 0.96 }}
          transition={{ duration: 0.4, ease: 'easeOut' }}
          className="onboarding"
        >
          <div className="onboarding__panel">
            <div className="onboarding__content">
              <div className="onboarding__header">
                <div className="onboarding__mark">
                  <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
                    <path
                      d="M3 5L7.5 9L3 13"
                      stroke="white"
                      strokeWidth="2"
                      strokeLinecap="round"
                      strokeLinejoin="round"
                    />
                    <path
                      d="M10 13H15"
                      stroke="currentColor"
                      strokeWidth="2"
                      strokeLinecap="round"
                    />
                  </svg>
                </div>
                <span className="onboarding__eyebrow">Welcome to Actio</span>
              </div>
              <p className="onboarding__title">
                Capture first, organize second.
              </p>
              <p className="onboarding__copy">
                Your board is set up for fast scanning, quick completion, and label-based focus. Start talking and refine later.
              </p>
              <button
                onClick={() => {
                  setVisible(false);
                  setHasSeenOnboarding(true);
                }}
                type="button"
                className="ghost-button onboarding__action"
              >
                Got it →
              </button>
            </div>
            <div className="onboarding__progress">
              <motion.div
                className="onboarding__progress-bar"
                style={{ width: `${progressWidth}%` }}
                transition={{ duration: 0.1 }}
              />
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
