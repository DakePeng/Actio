import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store/use-store';
import { getWsUrl } from '../api/backend-url';

interface ChatComposerProps {
  onClose: () => void;
}

export function ChatComposer({ onClose }: ChatComposerProps) {
  const extractReminders = useStore((s) => s.extractReminders);

  const [text, setText] = useState('');
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const wsRef = useRef<WebSocket | null>(null);

  // Auto-focus the textarea when the composer opens.
  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  // Auto-grow textarea up to ~12 lines.
  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 280)}px`;
  }, [text]);

  // Detach the WebSocket subscription on unmount.
  useEffect(() => {
    return () => {
      stopRecording();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // When the dictation hotkey fires while the composer is open, toggle the
  // local mic here instead of the global paste pipeline.
  const recordingRef = useRef(recording);
  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);
  useEffect(() => {
    const handler = () => {
      console.log('[Actio] Composer received dictation toggle, recording:', recordingRef.current);
      if (recordingRef.current) stopRecording();
      else startRecording();
    };
    window.addEventListener('actio-toggle-composer-dictation', handler);
    return () => window.removeEventListener('actio-toggle-composer-dictation', handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function stopRecording() {
    if (wsRef.current) {
      try {
        wsRef.current.close();
      } catch {
        /* ignore */
      }
      wsRef.current = null;
    }
    setRecording(false);
  }

  async function startRecording() {
    setError(null);
    // The backend runs an always-on inference pipeline. We only subscribe to
    // final transcript frames and append them into the note field.
    try {
      // Resolve via getWsUrl so port-fallback (3000-3009) works.
      const wsUrl = await getWsUrl('/ws');
      const ws = new WebSocket(wsUrl);
      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data);
          if (msg.kind === 'transcript' && msg.text && msg.is_final) {
            setText((prev) => (prev ? `${prev} ${msg.text}` : String(msg.text)));
          }
        } catch {
          /* ignore malformed frames */
        }
      };
      ws.onerror = () => {
        setError('Voice connection lost');
      };
      wsRef.current = ws;
      setRecording(true);
    } catch (e) {
      console.error('[Actio] chat ASR connect failed', e);
      setError(e instanceof Error ? e.message : 'Could not connect to voice input');
      wsRef.current = null;
    }
  }

  const toggleRecording = () => {
    if (recording) stopRecording();
    else startRecording();
  };

  const canSubmit = text.trim().length > 0;

  const handleSubmit = async () => {
    const content = text.trim();
    if (!content) return;
    setSubmitting(true);
    if (recording) stopRecording();

    try {
      void extractReminders(content);
      setText('');
      onClose();
    } finally {
      setSubmitting(false);
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      event.preventDefault();
      void handleSubmit();
    }
  };

  return (
    <div className="chat-composer">
      <div className="chat-composer__textwrap">
        <textarea
          ref={textareaRef}
          className="chat-composer__textarea"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={
            recording
              ? 'Listening - speak naturally, your words will appear here.'
              : 'Type a note or tap the mic to dictate.'
          }
          rows={2}
        />
      </div>

      {error && <div className="chat-composer__error">{error}</div>}

      <div className="chat-composer__bar">
        <div className="chat-composer__bar-left">
          <button
            type="button"
            className={`chat-composer__icon-btn${recording ? ' is-recording' : ''}`}
            onClick={toggleRecording}
            title={recording ? 'Stop dictation' : 'Dictate via microphone'}
            aria-label={recording ? 'Stop dictation' : 'Dictate via microphone'}
            aria-pressed={recording}
          >
            {recording ? (
              <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                <rect x="6" y="6" width="12" height="12" rx="2" />
              </svg>
            ) : (
              <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <line x1="12" x2="12" y1="19" y2="22" />
              </svg>
            )}
          </button>
          {recording && <span className="chat-composer__rec-pill">REC</span>}
        </div>
        <div className="chat-composer__bar-right">
          <span className="chat-composer__hint">Ctrl+Enter</span>
          <button
            type="button"
            className="primary-button chat-composer__send"
            disabled={!canSubmit || submitting}
            onClick={() => void handleSubmit()}
          >
            Send
          </button>
        </div>
      </div>
    </div>
  );
}
