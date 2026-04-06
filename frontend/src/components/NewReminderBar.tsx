import { useState, useRef, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../store/use-store';

export function NewReminderBar() {
  const show = useStore((s) => s.ui.showNewReminderBar);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const addReminder = useStore((s) => s.addReminder);

  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (show && inputRef.current) {
      inputRef.current.focus();
    }
  }, [show]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setNewReminderBar(false);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [setNewReminderBar]);

  const handleSubmit = () => {
    if (!title.trim()) return;
    addReminder({
      title: title.trim(),
      description: description.trim(),
      priority: 'medium',
      labels: [],
      createdAt: new Date().toISOString(),
    });
    setTitle('');
    setDescription('');
    setNewReminderBar(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      handleSubmit();
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
            onClick={() => setNewReminderBar(false)}
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
                  <div className="sheet-eyebrow">Manual capture</div>
                  <div className="sheet-title">Add a note without leaving the board</div>
                  <div className="sheet-copy">Keep the entry short. Triage and labeling can happen after capture.</div>
                </div>
                <div className="active-pill">Cmd/Ctrl + Enter to save</div>
              </div>
              <div className="quick-add__grid">
                <label>
                  <span className="field-label">Title</span>
                  <input
                    ref={inputRef}
                    type="text"
                    placeholder="What needs attention?"
                    value={title}
                    onChange={(e) => setTitle(e.target.value)}
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
                    onChange={(e) => setDescription(e.target.value)}
                    onKeyDown={handleKeyDown}
                    className="field-input"
                  />
                </label>
                <div className="quick-add__actions">
                  <button type="button" onClick={() => setNewReminderBar(false)} className="secondary-button">
                    Cancel
                  </button>
                  <button type="button" onClick={handleSubmit} disabled={!title.trim()} className="primary-button">
                    Add reminder
                  </button>
                </div>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
