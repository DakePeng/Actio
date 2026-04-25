import { useEffect, useMemo, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useStore } from '../store/use-store';
import { Board } from './Board';
import { NeedsReviewView } from './NeedsReviewView';
import { ArchiveView } from './ArchiveView';
import { SettingsView } from './settings/SettingsView';
import { LiveTab } from './LiveTab';
import { PeopleTab } from './PeopleTab';
import { TabBar } from './TabBar';
import { NewReminderBar } from './NewReminderBar';
import { useKeyboardShortcuts } from '../hooks/useKeyboardShortcuts';
import { useLanguage, useT, type TKey } from '../i18n';

// Dynamic greetings — one translation key per slot. The picker chooses a
// key at random each board open, then we translate it at render time so
// swapping language re-renders without re-picking.
const GREETING_KEYS: TKey[] = [
  'board.greeting.readyToAct',
  'board.greeting.getThingsDone',
  'board.greeting.agenda',
  'board.greeting.makeMoves',
  'board.greeting.boardAwaits',
  'board.greeting.whatsNext',
  'board.greeting.pickUp',
  'board.greeting.lockIn',
];

function formatGreeting(text: string, name: string, lang: 'en' | 'zh-CN'): string {
  if (!name) return text;
  if (lang === 'zh-CN') {
    // In Chinese we prepend the name with a comma, preserving trailing
    // punctuation: "准备好开始了吗？" → "Dake，准备好开始了吗？"
    return `${name}，${text}`;
  }
  // English: insert name before trailing punctuation.
  const punct = text.match(/[?!.]$/);
  if (punct) {
    return `${text.slice(0, -1)}, ${name}${punct[0]}`;
  }
  return `${text}, ${name}`;
}

type ExitTarget = { x: number; y: number; scale: number } | null;

export function BoardWindow() {
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const activeTab = useStore((s) => s.ui.activeTab);
  const profileName = useStore((s) => s.profile.name);
  const setBoardWindow = useStore((s) => s.setBoardWindow);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const clearFeedback = useStore((s) => s.clearFeedback);

  const { lang } = useLanguage();
  const t = useT();

  // Re-pick a greeting *key* each time the board opens. We keep the key —
  // not the rendered text — so flipping language mid-session still
  // translates without forcing a new random pick.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const greetingKey = useMemo<TKey>(
    () => GREETING_KEYS[Math.floor(Math.random() * GREETING_KEYS.length)],
    [showBoardWindow],
  );
  const greeting = formatGreeting(t(greetingKey), profileName, lang);

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
                  <div className="desktop-toolbar__title">{greeting}</div>
                </div>

                <TabBar />

                <div className="desktop-toolbar__actions">
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => void triggerClose()}
                  >
                    {t('board.action.returnToTray')}
                  </button>
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => setNewReminderBar(true)}
                  >
                    {t('board.action.captureNote')}
                  </button>
                </div>
              </div>

              <div className="desktop-window__body">
                {activeTab === 'board' && <Board />}
                {activeTab === 'needs-review' && <NeedsReviewView />}
                {activeTab === 'archive' && <ArchiveView />}
                {activeTab === 'settings' && <SettingsView />}
                {activeTab === 'live' && <LiveTab />}
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
