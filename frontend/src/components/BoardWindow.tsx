import { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { Board } from './Board';
import { ArchiveView } from './ArchiveView';
import { SettingsView } from './settings/SettingsView';
import { RecordingTab } from './RecordingTab';
import { PeopleTab } from './PeopleTab';
import { TabBar } from './TabBar';
import { NewReminderBar } from './NewReminderBar';

type ExitTarget = { x: number; y: number; scale: number } | null;

export function BoardWindow() {
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const activeTab = useStore((s) => s.ui.activeTab);
  const setBoardWindow = useStore((s) => s.setBoardWindow);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const clearFeedback = useStore((s) => s.clearFeedback);

  const windowRef = useRef<HTMLElement>(null);
  const [exitTarget, setExitTarget] = useState<ExitTarget>(null);
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  async function triggerClose() {
    clearFeedback();

    if (isTauri && windowRef.current) {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const { getCurrentWindow } = await import('@tauri-apps/api/window');

        const appWindow = getCurrentWindow();
        const [bounds, winPos] = await Promise.all([
          invoke<{ x: number; y: number; width: number; height: number }>('get_tray_bounds'),
          appWindow.outerPosition(),
        ]);

        const scaleFactor = await appWindow.scaleFactor();
        const winX = winPos.x / scaleFactor;
        const winY = winPos.y / scaleFactor;

        // Convert tray target from screen coords to window-local coords
        const targetX = bounds.x - winX;
        const targetY = bounds.y - winY;

        // Current board bounds in window-local coords
        const rect = windowRef.current.getBoundingClientRect();
        const boardCenterX = rect.left + rect.width / 2;
        const boardCenterY = rect.top + rect.height / 2;
        const targetCenterX = targetX + bounds.width / 2;
        const targetCenterY = targetY + bounds.height / 2;

        const scale = Math.max(
          bounds.width / rect.width,
          bounds.height / rect.height,
        );

        setExitTarget({
          x: targetCenterX - boardCenterX,
          y: targetCenterY - boardCenterY,
          scale,
        });
      } catch (err) {
        console.error('get_tray_bounds failed, using default exit', err);
      }
    }

    setBoardWindow(false);
  }

  useEffect(() => {
    if (!showBoardWindow) return;
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        void triggerClose();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [showBoardWindow]);

  // Reset exit target when we go back to shown
  useEffect(() => {
    if (showBoardWindow) setExitTarget(null);
  }, [showBoardWindow]);

  async function handleExitComplete() {
    if (!isTauri) return;
    // Swap body class now that exit animation is done
    document.body.classList.add('body--standby');
    document.body.classList.remove('body--native-board');

    const { invoke } = await import('@tauri-apps/api/core');
    const trayExpanded = useStore.getState().ui.trayExpanded;
    const reminderCount = useStore.getState().reminders.length;
    await invoke('sync_window_mode', {
      showBoard: false,
      trayExpanded,
      reminderCount,
      skipAnimation: true,
    });
  }

  const exitAnim = exitTarget
    ? {
        opacity: 0,
        x: exitTarget.x,
        y: exitTarget.y,
        scale: exitTarget.scale,
      }
    : { opacity: 0, y: 24, scale: 0.97 };

  const exitTransition = exitTarget
    ? { duration: 0.5, ease: [0.22, 1, 0.36, 1] as [number, number, number, number] }
    : { type: 'spring' as const, stiffness: 260, damping: 24 };

  return (
    <AnimatePresence onExitComplete={() => void handleExitComplete()}>
      {showBoardWindow && (
        <>
          <motion.div
            className="desktop-window-backdrop"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={() => void triggerClose()}
          />
          <div className="desktop-window-shell">
            <motion.section
              ref={windowRef}
              className="desktop-window"
              initial={{ opacity: 0, y: 36, scale: 0.94 }}
              animate={{ opacity: 1, y: 0, scale: 1 }}
              exit={exitAnim}
              transition={exitTransition}
              style={{ transformOrigin: 'center center' }}
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
                    onClick={() => void triggerClose()}
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
                {activeTab === 'recording' && <RecordingTab />}
                {activeTab === 'people' && <PeopleTab />}
              </div>
              <NewReminderBar />
            </motion.section>
          </div>
        </>
      )}
    </AnimatePresence>
  );
}
