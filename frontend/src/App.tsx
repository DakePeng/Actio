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

    // When opening board, swap body class and resize window immediately.
    // When closing, BoardWindow handles the class swap + window resize in its onExitComplete
    // after its exit animation plays.
    let cancelled = false;

    if (showBoardWindow) {
      document.body.classList.add('body--native-board');
      document.body.classList.remove('body--standby');

      const syncWindow = async () => {
        const { invoke } = await import('@tauri-apps/api/core');
        if (cancelled) return;
        await invoke('sync_window_mode', {
          showBoard: true,
          trayExpanded,
          reminderCount: reminders.length,
        });
      };
      void syncWindow();
    }

    return () => {
      cancelled = true;
    };
  }, [showBoardWindow, trayExpanded, reminders.length]);

  // On initial mount in Tauri: if we're starting in tray mode (the default),
  // ensure body--standby is applied so the tray's `position: fixed; inset: 0`
  // rule takes effect — otherwise the tray sizes to content and can clip
  // against the 320 logical-px window on the right edge. Also re-sync the
  // Tauri window size once the webview has rendered, which corrects any
  // initial DPI rounding on the Rust side. Fires exactly once per mount.
  useEffect(() => {
    const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
    if (!isTauri) return;
    if (showBoardWindow) return; // the board-mode effect above handles this case

    document.body.classList.add('body--standby');
    document.body.classList.remove('body--native-board');

    let cancelled = false;
    void (async () => {
      const { invoke } = await import('@tauri-apps/api/core');
      if (cancelled) return;
      await invoke('sync_window_mode', {
        showBoard: false,
        trayExpanded: useStore.getState().ui.trayExpanded,
        reminderCount: useStore.getState().reminders.length,
        skipAnimation: true,
      });
    })();

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []); // mount only

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
