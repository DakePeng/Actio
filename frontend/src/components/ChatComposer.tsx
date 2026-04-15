import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store/use-store';

const WS_BASE = 'ws://127.0.0.1:3000';
const MAX_IMAGE_BYTES = 4 * 1024 * 1024; // 4 MB per image

interface ChatComposerProps {
  onClose: () => void;
}

interface AttachedImage {
  id: string;
  name: string;
  dataUrl: string;
  sizeBytes: number;
}


function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === 'string' ? reader.result : '');
    reader.onerror = () => reject(reader.error ?? new Error('FileReader error'));
    reader.readAsDataURL(file);
  });
}

export function ChatComposer({ onClose }: ChatComposerProps) {
  const extractReminders = useStore((s) => s.extractReminders);

  const [text, setText] = useState('');
  const [images, setImages] = useState<AttachedImage[]>([]);
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const wsRef = useRef<WebSocket | null>(null);

  // Auto-focus the textarea when the composer opens
  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  // Auto-grow textarea up to ~12 lines
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

  function stopRecording() {
    if (wsRef.current) {
      try { wsRef.current.close(); } catch { /* ignore */ }
      wsRef.current = null;
    }
    setRecording(false);
  }

  function startRecording() {
    setError(null);
    // The backend runs an always-on inference pipeline. We just open a
    // WebSocket subscription to receive the live transcript stream — no
    // session creation, no backend state change. Closing the WS detaches us
    // but the pipeline keeps running for other consumers.
    try {
      const ws = new WebSocket(`${WS_BASE}/ws`);
      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data);
          if (msg.kind === 'transcript' && msg.text && msg.is_final) {
            // Append final transcripts to the textarea content
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

  const handleImagePick = async (e: React.ChangeEvent<HTMLInputElement>) => {
    setError(null);
    const files = e.target.files;
    if (!files || files.length === 0) return;
    const next: AttachedImage[] = [];
    for (const file of Array.from(files)) {
      if (file.size > MAX_IMAGE_BYTES) {
        setError(`${file.name} is larger than 4 MB`);
        continue;
      }
      try {
        const dataUrl = await fileToDataUrl(file);
        next.push({
          id: crypto.randomUUID(),
          name: file.name,
          dataUrl,
          sizeBytes: file.size,
        });
      } catch (err) {
        console.error('[Actio] image read failed', err);
      }
    }
    if (next.length > 0) setImages((prev) => [...prev, ...next]);
    if (fileInputRef.current) fileInputRef.current.value = ''; // allow re-picking same file
  };

  const removeImage = (id: string) => {
    setImages((prev) => prev.filter((img) => img.id !== id));
  };

  const canSubmit = text.trim().length > 0 || images.length > 0;

  const handleSubmit = async () => {
    const content = text.trim();
    if (!content && images.length === 0) return;
    setSubmitting(true);
    if (recording) stopRecording();

    try {
      if (content) {
        void extractReminders(content);
      }
      setText('');
      setImages([]);
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
              ? 'Listening… speak naturally, your words will appear here.'
              : 'Type a note, drop an image, or tap the mic to dictate.'
          }
          rows={2}
        />
      </div>

      {images.length > 0 && (
        <div className="chat-composer__attachments">
          {images.map((img) => (
            <div key={img.id} className="chat-attachment">
              <img src={img.dataUrl} alt={img.name} className="chat-attachment__thumb" />
              <button
                type="button"
                className="chat-attachment__remove"
                onClick={() => removeImage(img.id)}
                aria-label={`Remove ${img.name}`}
                title={`Remove ${img.name}`}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}

      {error && <div className="chat-composer__error">{error}</div>}

      <div className="chat-composer__bar">
        <div className="chat-composer__bar-left">
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*"
            multiple
            style={{ display: 'none' }}
            onChange={(e) => void handleImagePick(e)}
          />
          <button
            type="button"
            className="chat-composer__icon-btn"
            onClick={() => fileInputRef.current?.click()}
            title="Attach images"
            aria-label="Attach images"
          >
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M21.44 11.05 12.25 20.24a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48" />
            </svg>
          </button>
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
          <span className="chat-composer__hint">⌘/Ctrl+Enter</span>
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
