import { createContext, useEffect, useMemo, useState } from 'react';
import { motion } from 'framer-motion';
import { useStore } from '../store/use-store';
import { sortByPriority } from '../utils/priority';
import { formatTimeShort } from '../utils/time';

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
  const newCount = reminders.filter((r) => r.isNew).length;
  const [expanded, setExpanded] = useState(false);
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  const topReminders = useMemo(() => {
    return [...reminders].sort(sortByPriority).slice(0, 6);
  }, [reminders]);

  useEffect(() => {
    setTrayExpanded(expanded);
  }, [expanded, setTrayExpanded]);

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
          {topReminders.map((reminder, index) => (
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
          ))}

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
