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
              <div style={{ display: 'flex', alignItems: 'center', gap: '12px', marginBottom: '12px' }}>
                <div
                  style={{
                    width: '40px',
                    height: '40px',
                    borderRadius: '14px',
                    background: 'linear-gradient(135deg, var(--color-accent), #d68b4f)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    color: 'white',
                  }}
                >
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
                <span style={{ fontSize: '0.88rem', fontWeight: 700, color: 'var(--color-text)' }}>Welcome to Actio</span>
              </div>
              <p style={{ fontSize: '1.35rem', fontWeight: 700, color: 'var(--color-text)', marginBottom: '6px', letterSpacing: '-0.04em' }}>
                Capture first, organize second.
              </p>
              <p style={{ fontSize: '0.95rem', color: 'var(--color-text-secondary)', lineHeight: 1.6 }}>
                Your board is set up for fast scanning, quick completion, and label-based focus. Start talking and refine later.
              </p>
              <button
                onClick={() => {
                  setVisible(false);
                  setHasSeenOnboarding(true);
                }}
                type="button"
                className="ghost-button"
                style={{ marginTop: '16px', padding: 0, height: 'auto', color: 'var(--color-accent-strong)', fontWeight: 700 }}
              >
                Got it →
              </button>
            </div>
            <div className="onboarding__progress">
              <motion.div
                style={{ height: '100%', width: `${progressWidth}%`, background: 'rgba(190, 91, 49, 0.55)' }}
                transition={{ duration: 0.1 }}
              />
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
