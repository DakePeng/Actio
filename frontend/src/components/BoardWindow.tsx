import { useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { Board } from './Board';
import { ArchiveView } from './ArchiveView';
import { SettingsView } from './settings/SettingsView';
import { TabBar } from './TabBar';
import { NewReminderBar } from './NewReminderBar';

export function BoardWindow() {
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const activeTab = useStore((s) => s.ui.activeTab);
  const setBoardWindow = useStore((s) => s.setBoardWindow);
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

                <TabBar />

                <div className="desktop-toolbar__actions">
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
                  <button
                    type="button"
                    className="primary-button"
                    disabled={activeTab !== 'board'}
                    onClick={() => setNewReminderBar(true)}
                  >
                    Capture note
                  </button>
                </div>
              </div>

              <div className="desktop-window__body">
                {activeTab === 'board' && <Board />}
                {activeTab === 'archive' && <ArchiveView />}
                {activeTab === 'settings' && <SettingsView />}
              </div>
              <NewReminderBar />
            </motion.section>
          </div>
        </>
      )}
    </AnimatePresence>
  );
}
