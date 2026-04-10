import { useEffect } from 'react';
import { useStore } from './store/use-store';
import { BoardWindow } from './components/BoardWindow';
import { FeedbackToast } from './components/FeedbackToast';
import { StandbyTray } from './components/StandbyTray';
import { OnboardingCard } from './components/OnboardingCard';

export default function App() {
  const hasSeenOnboarding = useStore((s) => s.ui.hasSeenOnboarding);
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const trayExpanded = useStore((s) => s.ui.trayExpanded);
  const reminders = useStore((s) => s.reminders);
  const loadBoard = useStore((s) => s.loadBoard);
  const theme = useStore((s) => s.preferences.theme);

  useEffect(() => {
    void loadBoard();
  }, [loadBoard]);

  useEffect(() => {
    const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
    if (!isTauri) return;

    document.body.classList.toggle('body--standby', !showBoardWindow);
    document.body.classList.toggle('body--native-board', showBoardWindow);

    let cancelled = false;

    const syncWindow = async () => {
      const [{ invoke }] = await Promise.all([import('@tauri-apps/api/core')]);

      if (cancelled) return;

      await invoke('sync_window_mode', {
        showBoard: showBoardWindow,
        trayExpanded,
        reminderCount: reminders.length,
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
    const root = document.documentElement;
    if (theme === 'system') {
      root.removeAttribute('data-theme');
    } else {
      root.setAttribute('data-theme', theme);
    }
  }, [theme]);

  return (
    <div className={`app-shell${showBoardWindow ? '' : ' app-shell--standby'}`}>
      <StandbyTray />
      <BoardWindow />
      <FeedbackToast />
      {showBoardWindow && !hasSeenOnboarding && <OnboardingCard />}
    </div>
  );
}
