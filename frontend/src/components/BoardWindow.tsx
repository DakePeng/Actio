import { useEffect, useMemo, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { Board } from './Board';
import { ArchiveView } from './ArchiveView';
import { SettingsView } from './settings/SettingsView';
import { RecordingTab } from './RecordingTab';
import { PeopleTab } from './PeopleTab';
import { TabBar } from './TabBar';
import { NewReminderBar } from './NewReminderBar';
import { useKeyboardShortcuts } from '../hooks/useKeyboardShortcuts';

// Dynamic greetings — picked randomly each time the board opens.
// Each entry has a `text` and an optional `nameStyle`:
//   'suffix'  → "Ready to act, Dake?"
//   'none'    → shown as-is regardless of name
const GREETINGS: { text: string; nameStyle: 'suffix' | 'none' }[] = [
  { text: 'Ready to act?', nameStyle: 'suffix' },
  { text: "Let's get things done", nameStyle: 'suffix' },
  { text: "What's on the agenda?", nameStyle: 'suffix' },
  { text: 'Time to make moves', nameStyle: 'suffix' },
  { text: 'Your board awaits', nameStyle: 'suffix' },
  { text: "What's next?", nameStyle: 'suffix' },
  { text: 'Pick up where you left off', nameStyle: 'suffix' },
  { text: "Let's lock in", nameStyle: 'suffix' },
];

function pickGreeting(name: string): string {
  const entry = GREETINGS[Math.floor(Math.random() * GREETINGS.length)];
  if (entry.nameStyle === 'suffix' && name) {
    // Insert name before trailing punctuation: "Ready to act, Dake?"
    const punct = entry.text.match(/[?!.]$/);
    if (punct) {
      return `${entry.text.slice(0, -1)}, ${name}${punct[0]}`;
    }
    return `${entry.text}, ${name}`;
  }
  return entry.text;
}

type ExitTarget = { x: number; y: number; scale: number } | null;

export function BoardWindow() {
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const activeTab = useStore((s) => s.ui.activeTab);
  const profileName = useStore((s) => s.profile.name);
  const setBoardWindow = useStore((s) => s.setBoardWindow);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const clearFeedback = useStore((s) => s.clearFeedback);

  // Re-pick a greeting each time the board opens
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const greeting = useMemo(() => pickGreeting(profileName), [profileName, showBoardWindow]);

  useKeyboardShortcuts();

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

    // Kick off the window snap + tray fade-in partway through the exit animation.
    // At 280ms the board is ~93% shrunk (cubic ease-out) — close enough that snapping
    // the window and revealing the tray doesn't produce visual tearing.
    if (isTauri) {
      setTimeout(async () => {
        const { invoke } = await import('@tauri-apps/api/core');
        const trayExpanded = useStore.getState().ui.trayExpanded;
        const reminderCount = useStore.getState().reminders.length;
        await invoke('sync_window_mode', {
          showBoard: false,
          trayExpanded,
          reminderCount,
          skipAnimation: true,
        });
        // Swap body class immediately after snap — tray starts fading in now
        document.body.classList.add('body--standby');
        document.body.classList.remove('body--native-board');
      }, 280);
    }
  }

  // Reset exit target when we go back to shown
  useEffect(() => {
    if (showBoardWindow) setExitTarget(null);
  }, [showBoardWindow]);

  function handleExitComplete() {
    // Window snap + body class swap are already handled in triggerClose's setTimeout.
    // This is a safety net in case the timeout hasn't fired (e.g., fast close).
    if (!isTauri) return;
    document.body.classList.add('body--standby');
    document.body.classList.remove('body--native-board');
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
                    <div className="desktop-toolbar__title">{greeting}</div>
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
