import { useState, useRef, useEffect } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { useStore } from '../store/use-store';
import { ChatComposer } from './ChatComposer';
import { useT } from '../i18n';

type CaptureMode = 'chat' | 'form';
const MODE_STORAGE_KEY = 'actio-capture-mode';
const SETTINGS_API_URL = `${(import.meta.env.VITE_ACTIO_API_BASE_URL ?? 'http://127.0.0.1:3000').replace(/\/$/, '')}/settings`;

function isLlmConfigured(settings: unknown): boolean {
  const selection = (settings as { llm?: { selection?: { kind?: string } } })?.llm?.selection;
  return Boolean(selection?.kind && selection.kind !== 'disabled');
}

async function fetchLlmConfigured(signal: AbortSignal): Promise<boolean> {
  try {
    const response = await fetch(SETTINGS_API_URL, { signal });
    if (!response.ok) return false;
    return isLlmConfigured(await response.json());
  } catch {
    return false;
  }
}

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
  const setFeedback = useStore((s) => s.setFeedback);

  const [mode, setMode] = useState<CaptureMode>(loadInitialMode);
  const t = useT();

  // Persist mode preference across sessions
  useEffect(() => {
    try {
      localStorage.setItem(MODE_STORAGE_KEY, mode);
    } catch {
      /* ignore quota errors */
    }
  }, [mode]);

  useEffect(() => {
    if (!show || mode !== 'chat') return;

    let cancelled = false;
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 800);

    void fetchLlmConfigured(controller.signal).then((configured) => {
      if (cancelled || configured) return;
      setMode('form');
      setFeedback('feedback.llmNotConfiguredFormMode');
    });

    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [show, mode, setFeedback]);

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
                  <div className="sheet-eyebrow">{t('newReminder.quickCapture')}</div>
                  <div className="sheet-title">
                    {mode === 'chat'
                      ? t('newReminder.title.chat')
                      : t('newReminder.title.form')}
                  </div>
                  <div className="sheet-copy">
                    {mode === 'chat'
                      ? t('newReminder.copy.chat')
                      : t('newReminder.copy.form')}
                  </div>
                </div>
                <div className="quick-add__header-actions">
                  <div className="active-pill">{t('newReminder.saveHint')}</div>
                  <button
                    type="button"
                    className="quick-add__close"
                    onClick={handleClose}
                    aria-label={t('newReminder.aria.close')}
                    title={t('newReminder.tooltip.close')}
                  >
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M18 6 6 18" />
                      <path d="m6 6 12 12" />
                    </svg>
                  </button>
                </div>
              </div>

              {mode === 'chat' ? (
                <ChatComposer onClose={handleClose} />
              ) : (
                <div className="quick-add__grid">
                  <label>
                    <span className="field-label">{t('newReminder.field.title')}</span>
                    <input
                      ref={inputRef}
                      type="text"
                      placeholder={t('newReminder.placeholder.title')}
                      value={title}
                      onChange={(event) => setTitle(event.target.value)}
                      onKeyDown={handleKeyDown}
                      className="field-input"
                    />
                  </label>
                  <label>
                    <span className="field-label">{t('newReminder.field.details')}</span>
                    <textarea
                      rows={2}
                      placeholder={t('newReminder.placeholder.details')}
                      value={description}
                      onChange={(event) => setDescription(event.target.value)}
                      onKeyDown={handleKeyDown}
                      className="field-input"
                    />
                  </label>
                  <label>
                    <span className="field-label">{t('newReminder.field.dueTime')}</span>
                    <input
                      type="text"
                      placeholder={t('newReminder.placeholder.dueTime')}
                      value={dueTime}
                      onChange={(event) => setDueTime(event.target.value)}
                      onKeyDown={handleKeyDown}
                      className="field-input"
                    />
                  </label>
                  <div className="quick-add__actions">
                    <button type="button" onClick={handleClose} className="secondary-button">
                      {t('newReminder.cancel')}
                    </button>
                    <button
                      type="button"
                      onClick={() => void handleSubmit()}
                      disabled={!title.trim()}
                      className="primary-button"
                    >
                      {t('newReminder.addReminder')}
                    </button>
                  </div>
                </div>
              )}
              <div className="quick-add__mode-switch-row">
                <button
                  type="button"
                  className="quick-add__mode-toggle"
                  onClick={() => setMode(mode === 'chat' ? 'form' : 'chat')}
                  title={
                    mode === 'chat'
                      ? t('newReminder.tooltip.switchToForm')
                      : t('newReminder.tooltip.switchToChat')
                  }
                >
                  {mode === 'chat'
                    ? t('newReminder.switchToForm')
                    : t('newReminder.switchToChat')}
                </button>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
