import { useEffect } from 'react';
import { useStore } from './store/use-store';
import { BoardWindow } from './components/BoardWindow';
import { FeedbackToast } from './components/FeedbackToast';
import { StandbyTray } from './components/StandbyTray';
import { OnboardingCard } from './components/OnboardingCard';
import { MOCK_REMINDERS } from './tauri/mock-data';
import type { Reminder } from './types';

const STANDBY_POSITION_KEY = 'actio-standby-position';
const STANDBY_POSITION_PINNED_KEY = 'actio-standby-position-pinned';
const STANDBY_POSITION_VERSION = 2;

function readStandbyPosition() {
  if (typeof window === 'undefined') return null;

  const raw = localStorage.getItem(STANDBY_POSITION_KEY);
  if (!raw) return null;

  try {
    const parsed = JSON.parse(raw) as { x: number; y: number; version?: number };
    if (
      parsed.version === STANDBY_POSITION_VERSION &&
      typeof parsed.x === 'number' &&
      typeof parsed.y === 'number'
    ) {
      return parsed;
    }
  } catch {}

  return null;
}

export default function App() {
  const hasSeenOnboarding = useStore((s) => s.ui.hasSeenOnboarding);
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const trayExpanded = useStore((s) => s.ui.trayExpanded);
  const reminders = useStore((s) => s.reminders);
  const setReminders = useStore((s) => s.setReminders);

  useEffect(() => {
    let cancelled = false;

    const loadReminders = async () => {
      try {
        const response = await fetch('http://localhost:3001/reminders');
        if (!response.ok) {
          throw new Error(`Mock API returned ${response.status}`);
        }
        const reminders = (await response.json()) as Reminder[];
        if (!cancelled) {
          setReminders(reminders);
        }
      } catch {
        if (!cancelled) {
          setReminders(MOCK_REMINDERS);
        }
      }
    };

    void loadReminders();

    return () => {
      cancelled = true;
    };
  }, [setReminders]);

  useEffect(() => {
    const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
    if (!isTauri) return;

    document.body.classList.toggle('body--standby', !showBoardWindow);
    document.body.classList.toggle('body--native-board', showBoardWindow);

    let cancelled = false;

    const syncWindow = async () => {
      const [{ invoke }] = await Promise.all([import('@tauri-apps/api/core')]);

      if (cancelled) return;
      const isPinned = localStorage.getItem(STANDBY_POSITION_PINNED_KEY) === 'true';
      const standbyPosition = isPinned ? readStandbyPosition() : null;

      await invoke('sync_window_mode', {
        showBoard: showBoardWindow,
        trayExpanded,
        reminderCount: reminders.length,
        standbyPosition,
      });
    };

    void syncWindow();

    return () => {
      cancelled = true;
      document.body.classList.remove('body--standby');
      document.body.classList.remove('body--native-board');
    };
  }, [showBoardWindow, trayExpanded, reminders.length]);

  useEffect(() => {
    const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
    if (!isTauri) return;

    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const watchWindowMoves = async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      if (cancelled) return;

      unlisten = await getCurrentWindow().onMoved(({ payload }) => {
        if (showBoardWindow) return;
        localStorage.setItem(
          STANDBY_POSITION_KEY,
          JSON.stringify({ x: payload.x, y: payload.y, version: STANDBY_POSITION_VERSION }),
        );
      });
    };

    void watchWindowMoves();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [showBoardWindow]);

  return (
    <div className={`app-shell${showBoardWindow ? '' : ' app-shell--standby'}`}>
      {showBoardWindow && (
        <div className="desktop-stage">
          <div className="ambient-orb ambient-orb--left" />
          <div className="ambient-orb ambient-orb--right" />
        </div>
      )}
      <StandbyTray />
      <BoardWindow />
      <FeedbackToast />
      {showBoardWindow && !hasSeenOnboarding && <OnboardingCard />}
    </div>
  );
}
