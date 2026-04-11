import { createContext, useEffect, useMemo, useRef, useState } from 'react';
import { motion } from 'framer-motion';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useStore } from '../store/use-store';
import { sortByPriority } from '../utils/priority';
import { formatTimeShort } from '../utils/time';
import { SwipeActionRow } from './swipe/SwipeActionRow';
import { SwipeActionCoordinatorProvider } from './swipe/SwipeActionCoordinator';

// Context to share tray state with FAB
export const StandbyTrayContext = createContext({ expanded: false });

const STANDBY_COLLAPSED_WIDTH = 320;
const STANDBY_EXPANDED_WIDTH = 440;

export function StandbyTray() {
  const reminders = useStore((s) => s.reminders);
  const showBoardWindow = useStore((s) => s.ui.showBoardWindow);
  const setBoardWindow = useStore((s) => s.setBoardWindow);
  const setTrayExpanded = useStore((s) => s.setTrayExpanded);
  const setExpandedCard = useStore((s) => s.setExpandedCard);
  const highlightCard = useStore((s) => s.highlightCard);
  const archiveReminder = useStore((s) => s.archiveReminder);
  const newCount = reminders.filter((r) => r.isNew && r.archivedAt === null).length;
  const [expanded, setExpanded] = useState(false);
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  const topReminders = useMemo(() => {
    return [...reminders].filter((r) => r.archivedAt === null).sort(sortByPriority).slice(0, 6);
  }, [reminders]);

  useEffect(() => {
    setTrayExpanded(expanded);
  }, [expanded, setTrayExpanded]);

  // Resize the Tauri window to match the tray state. The framer-motion
  // width animation on the tray element grows the DOM to 440px when
  // expanded, but the Tauri window stays at 320px unless we explicitly
  // sync it — without this, expansion is clipped by the window edge.
  //
  // Also fires when the reminder count changes while the tray is
  // expanded, so the window grows/shrinks to fit the visible list.
  //
  // CRITICAL skips:
  //   - Board mode (showBoardWindow=true): React hooks run regardless of
  //     the `if (showBoardWindow) return null` render guard, so without
  //     this check, adding a reminder in board mode would trigger
  //     sync_window_mode(showBoard=false) and shrink the window mid-board.
  //   - Board → tray transition (the frame where showBoardWindow flips
  //     from true to false): BoardWindow.triggerClose owns this exit and
  //     coordinates a 280ms-delayed sync with the framer-motion shrink
  //     animation. Firing our own sync early breaks the exit animation
  //     and races on the saved_position read, sometimes landing the tray
  //     in the upper-left of the screen.
  //
  // The mount-time run uses skipAnimation:true so the window is at the
  // correct size immediately (matching the initial framer-motion state
  // and avoiding a jump). Subsequent user toggles animate.
  const mountedRef = useRef(false);
  const prevShowBoardRef = useRef(showBoardWindow);
  useEffect(() => {
    const wasBoardMode = prevShowBoardRef.current;
    prevShowBoardRef.current = showBoardWindow;

    if (!isTauri) return;
    if (showBoardWindow) return; // board mode owns the window
    if (wasBoardMode) return; // just transitioned from board — triggerClose owns the exit

    const isMount = !mountedRef.current;
    mountedRef.current = true;

    invoke('sync_window_mode', {
      showBoard: false,
      trayExpanded: expanded,
      reminderCount: topReminders.length,
      skipAnimation: isMount,
    }).catch((e) => console.warn('[Actio] tray sync_window_mode failed', e));
  }, [expanded, topReminders.length, isTauri, showBoardWindow]);

  function handleDragStart() {
    if (!isTauri) return;

    const appWindow = getCurrentWindow();

    // Native OS drag — must be called synchronously during mousedown
    appWindow.startDragging();

    // Save position after drag stops (debounced via onMoved)
    let debounceTimer: number | null = null;
    let unlistenFn: (() => void) | null = null;

    appWindow.onMoved(() => {
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = window.setTimeout(() => {
        invoke('save_tray_position');
        if (unlistenFn) unlistenFn();
      }, 200);
    }).then((fn) => { unlistenFn = fn; });
  }

  if (showBoardWindow) return null;

  const priorityDotColor = (p: string) =>
    p === 'high' ? '#DC2626' : p === 'medium' ? '#D97706' : '#16A34A';

  return (
    <StandbyTrayContext.Provider value={{ expanded }}>
      <motion.div
        className={`tray tray--launcher${expanded ? ' tray--hovered' : ' tray--collapsed'}`}
        initial={false}
        animate={{
          width: expanded ? STANDBY_EXPANDED_WIDTH : STANDBY_COLLAPSED_WIDTH,
          y: expanded ? -2 : 0,
          boxShadow: isTauri ? 'none' : expanded ? 'var(--shadow-card-lg)' : 'var(--shadow-card-md)',
        }}
        transition={{
          width: { duration: 0.3, ease: [0.22, 1, 0.36, 1] },
          y: { duration: 0.24, ease: 'easeOut' },
          boxShadow: { duration: 0.24, ease: 'easeOut' },
        }}
      >
        {/* Drag handle */}
        <div
          className="tray-drag-handle"
          onMouseDown={handleDragStart}
          role="separator"
          aria-label="Drag to reposition"
        >
          <div className="tray-drag-pill" />
        </div>
        {newCount > 0 && <span className="tray-badge">{newCount > 9 ? '9+' : newCount}</span>}
        <div className="tray-toggle">
          <button type="button" className="tray-brand-trigger" onClick={() => setExpanded((prev) => !prev)}>
            <div className="tray-brand">
              <span className="tray-brand-dot" aria-hidden="true" />
              <div>
                <div className="tray-brand-name">actio</div>
                <div className="tray-brand-subtitle">
                  {newCount > 0 ? `${newCount} fresh captures waiting` : 'Quiet queue, board ready'}
                </div>
              </div>
            </div>
          </button>
          <button
            type="button"
            className="tray-chevron-button"
            aria-label="Open board"
            onClick={() => {
              setExpanded(false);
              setBoardWindow(true);
            }}
          >
            <svg
              className="tray-launch-icon"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.8"
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden="true"
            >
              <path d="M14 5h5v5" />
              <path d="M10 14 19 5" />
              <path d="M19 14v4a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h4" />
            </svg>
          </button>
        </div>

        <motion.div
          className="tray-list"
          initial={false}
          animate={{
            height: expanded ? 'auto' : 0,
            opacity: expanded ? 1 : 0,
          }}
          transition={{
            height: { duration: 0.3, ease: [0.22, 1, 0.36, 1] },
            opacity: { duration: expanded ? 0.24 : 0.16, ease: 'easeOut' },
          }}
          style={{ pointerEvents: expanded ? 'auto' : 'none' }}
        >
          <SwipeActionCoordinatorProvider>
          {topReminders.map((reminder, index) => (
            <SwipeActionRow
              key={reminder.id}
              rowId={reminder.id}
              rightAction={{ label: 'Done', confirmLabel: 'Confirm', onExecute: () => archiveReminder(reminder.id) }}
            >
            <motion.button
              key={reminder.id}
              type="button"
              className="tray-item"
              initial={false}
              animate={{
                opacity: expanded ? 1 : 0,
                x: expanded ? 0 : 8,
              }}
              transition={{ duration: 0.2, delay: expanded ? 0.04 + index * 0.025 : 0 }}
              onClick={() => {
                setExpandedCard(reminder.id);
                highlightCard(reminder.id);
                setExpanded(false);
                setBoardWindow(true);
              }}
            >
              <div className="tray-item-header">
                <span className="tray-item-priority" style={{ background: priorityDotColor(reminder.priority) }} />
                <span className="tray-item-title">{reminder.title}</span>
                {reminder.dueTime && <span className="tray-item-time">{formatTimeShort(reminder.dueTime)}</span>}
              </div>
            </motion.button>
            </SwipeActionRow>
          ))}
          </SwipeActionCoordinatorProvider>

          <motion.div
            className="tray-cta"
            initial={false}
            animate={{
              opacity: expanded ? 1 : 0,
              y: expanded ? 0 : 6,
            }}
            transition={{ duration: 0.2, delay: expanded ? 0.12 : 0 }}
          >
            <button
              type="button"
              className="primary-button"
              onClick={() => {
                setExpanded(false);
                setBoardWindow(true);
              }}
            >
              View full board
            </button>
          </motion.div>
        </motion.div>
      </motion.div>
    </StandbyTrayContext.Provider>
  );
}
