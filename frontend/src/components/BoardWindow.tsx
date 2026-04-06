import { useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { Board } from './Board';
import { LabelsPanel } from './LabelsPanel';
import { NewReminderBar } from './NewReminderBar';

export function BoardWindow() {
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const search = useStore((s) => s.filter.search);
  const activeLabel = useStore((s) => s.filter.label);
  const setFilter = useStore((s) => s.setFilter);
  const setBoardWindow = useStore((s) => s.setBoardWindow);
  const toggleLabelsPanel = useStore((s) => s.toggleLabelsPanel);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const clearFeedback = useStore((s) => s.clearFeedback);

  useEffect(() => {
    if (!showBoardWindow) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setBoardWindow(false);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [showBoardWindow, setBoardWindow]);

  return (
    <AnimatePresence>
      {showBoardWindow && (
        <>
          <motion.div
            className="desktop-window-backdrop"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => {
              clearFeedback();
              setBoardWindow(false);
            }}
          />
          <div className="desktop-window-shell">
            <motion.section
              className="desktop-window"
              initial={{ opacity: 0, y: 36, scale: 0.94 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={{ opacity: 0, y: 24, scale: 0.97 }}
              transition={{ type: 'spring', stiffness: 260, damping: 24 }}
            >
              <div className="desktop-toolbar">
                <div className="desktop-toolbar__brand">
                  <div>
                    <div className="desktop-toolbar__title">Actio board</div>
                  </div>
                </div>

                <div className="desktop-toolbar__actions">
                  <div className="search-shell desktop-toolbar__search">
                    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                      <circle cx="11" cy="11" r="7" />
                      <path d="m21 21-4.3-4.3" />
                    </svg>
                    <input
                      className="search-input"
                      type="text"
                      placeholder="Search reminders"
                      value={search}
                      onChange={(e) => setFilter({ search: e.target.value })}
                    />
                  </div>
                  <button
                    type="button"
                    className={`pill-button${activeLabel ? ' is-active' : ''}`}
                    onClick={toggleLabelsPanel}
                  >
                    {activeLabel ? 'Label active' : 'Browse labels'}
                  </button>
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => {
                      clearFeedback();
                      setBoardWindow(false);
                    }}
                  >
                    Return to tray
                  </button>
                  <button type="button" className="primary-button" onClick={() => setNewReminderBar(true)}>
                    Capture note
                  </button>
                </div>
              </div>

              <div className="desktop-window__body">
                <Board />
              </div>

              <LabelsPanel />
              <NewReminderBar />
            </motion.section>
          </div>
        </>
      )}
    </AnimatePresence>
  );
}
