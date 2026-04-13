import { useState, useRef, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../store/use-store';
import { ChatComposer } from './ChatComposer';

type CaptureMode = 'chat' | 'form';
const MODE_STORAGE_KEY = 'actio-capture-mode';

function loadInitialMode(): CaptureMode {
  try {
    const saved = localStorage.getItem(MODE_STORAGE_KEY);
    if (saved === 'chat' || saved === 'form') return saved;
  } catch {
    /* ignore */
  }
  return 'chat'; // chat is the new default
}

export function NewReminderBar() {
  const show = useStore((s) => s.ui.showNewReminderBar);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const addReminder = useStore((s) => s.addReminder);

  const [mode, setMode] = useState<CaptureMode>(loadInitialMode);

  // Persist mode preference across sessions
  useEffect(() => {
    try {
      localStorage.setItem(MODE_STORAGE_KEY, mode);
    } catch {
      /* ignore quota errors */
    }
  }, [mode]);

  // Form mode state (only used when mode === 'form')
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [dueTime, setDueTime] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (show && mode === 'form' && inputRef.current) {
      inputRef.current.focus();
    }
  }, [show, mode]);

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        setNewReminderBar(false);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [setNewReminderBar]);

  const handleClose = () => {
    setTitle('');
    setDescription('');
    setDueTime('');
    setNewReminderBar(false);
  };

  const handleSubmit = async () => {
    if (!title.trim()) return;
    await addReminder({
      title: title.trim(),
      description: description.trim(),
      dueTime: dueTime.trim() || undefined,
      priority: 'medium',
      labels: [],
      createdAt: new Date().toISOString(),
      archivedAt: null,
    });
    setTitle('');
    setDescription('');
    setDueTime('');
    setNewReminderBar(false);
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      event.preventDefault();
      void handleSubmit();
    }
  };

  return (
    <AnimatePresence>
      {show && (
        <>
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="sheet-overlay"
            onClick={handleClose}
          />
          <motion.div
            initial={{ y: '100%' }}
            animate={{ y: 0 }}
            exit={{ y: '100%' }}
            transition={{ type: 'spring', damping: 25, stiffness: 200 }}
            className="quick-add"
          >
            <div className="quick-add__panel">
              <div className="sheet-header quick-add__header">
                <div>
                  <div className="sheet-eyebrow">Quick capture</div>
                  <div className="sheet-title">
                    {mode === 'chat'
                      ? 'Type, dictate, or attach an image'
                      : 'Add a note without leaving the board'}
                  </div>
                  <div className="sheet-copy">
                    {mode === 'chat'
                      ? 'Free-form note. Triage and labeling can happen after capture.'
                      : 'Keep the entry short. Triage and labeling can happen after capture.'}
                  </div>
                </div>
                <div className="quick-add__header-actions">
                  <button
                    type="button"
                    className="quick-add__mode-toggle"
                    onClick={() => setMode(mode === 'chat' ? 'form' : 'chat')}
                    title={mode === 'chat' ? 'Switch to form view' : 'Switch to chat view'}
                  >
                    {mode === 'chat' ? 'Switch to form' : 'Switch to chat'}
                  </button>
                  <div className="active-pill">Cmd/Ctrl + Enter to save</div>
                </div>
              </div>

              {mode === 'chat' ? (
                <ChatComposer onClose={handleClose} />
              ) : (
                <div className="quick-add__grid">
                  <label>
                    <span className="field-label">Title</span>
                    <input
                      ref={inputRef}
                      type="text"
                      placeholder="What needs attention?"
                      value={title}
                      onChange={(event) => setTitle(event.target.value)}
                      onKeyDown={handleKeyDown}
                      className="field-input"
                    />
                  </label>
                  <label>
                    <span className="field-label">Details</span>
                    <textarea
                      rows={2}
                      placeholder="Optional context, owner, or timing"
                      value={description}
                      onChange={(event) => setDescription(event.target.value)}
                      onKeyDown={handleKeyDown}
                      className="field-input"
                    />
                  </label>
                  <label>
                    <span className="field-label">Due time</span>
                    <input
                      type="text"
                      placeholder="e.g. 2026-04-09T18:30:00Z"
                      value={dueTime}
                      onChange={(event) => setDueTime(event.target.value)}
                      onKeyDown={handleKeyDown}
                      className="field-input"
                    />
                  </label>
                  <div className="quick-add__actions">
                    <button type="button" onClick={handleClose} className="secondary-button">
                      Cancel
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleSubmit()}
                      disabled={!title.trim()}
                      className="primary-button"
                    >
                      Add reminder
                    </button>
                  </div>
                </div>
              )}
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
