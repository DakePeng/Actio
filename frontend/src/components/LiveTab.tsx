import { useEffect, useRef, useState } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';
import { LiveTranscript } from './LiveTranscript';
import { ListeningToggle } from './ListeningToggle';
import { useT } from '../i18n';

const TRANSLATE_LANGS = ['en', 'zh-CN', 'ja', 'es', 'fr', 'de'] as const;

function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}

export function LiveTab() {
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);
  const listeningStartedAt = useStore((s) => s.ui.listeningStartedAt);
  const currentSession = useVoiceStore((s) => s.currentSession);
  const translation = useVoiceStore((s) => s.translation);
  const setTranslationEnabled = useVoiceStore((s) => s.setTranslationEnabled);
  const setTranslationTargetLang = useVoiceStore((s) => s.setTranslationTargetLang);
  const t = useT();

  const transcriptRef = useRef<HTMLDivElement>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!listeningEnabled || !listeningStartedAt) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [listeningEnabled, listeningStartedAt]);

  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [currentSession?.lines, currentSession?.pendingPartial]);

  const isOn = listeningEnabled === true;
  const headerLabel = isOn ? t('live.header.on') : t('live.header.off');

  return (
    <div className="live-tab">
      <div className="live-tab__header">
        <span className={`live-tab__status${isOn ? ' is-on' : ''}`}>{headerLabel}</span>
        <div className="live-tab__translate-cluster">
          <button
            type="button"
            className={`live-tab__translate-toggle${translation.enabled ? ' is-on' : ''}`}
            aria-pressed={translation.enabled}
            onClick={() => void setTranslationEnabled(!translation.enabled)}
          >
            {t('live.translate.toggle')}
          </button>
          <select
            className="live-tab__translate-select"
            aria-label={t('live.translate.targetLabel')}
            value={translation.targetLang}
            disabled={!translation.enabled}
            onChange={(e) => void setTranslationTargetLang(e.target.value)}
          >
            {TRANSLATE_LANGS.map((lang) => (
              <option key={lang} value={lang}>
                {t(`live.translate.lang.${lang}` as Parameters<typeof t>[0])}
              </option>
            ))}
          </select>
        </div>
        <ListeningToggle size={32} />
      </div>

      {isOn && listeningStartedAt && (
        <p className="live-tab__since" aria-live="polite">
          {t('live.listeningSince', {
            time: formatTime(listeningStartedAt),
            duration: formatDuration(now - listeningStartedAt),
          })}
        </p>
      )}

      {!isOn && (
        <p className="live-tab__paused-hint">{t('live.pausedHint')}</p>
      )}

      {isOn && currentSession && (
        <div className="live-tab__transcript" ref={transcriptRef} aria-live="polite">
          <LiveTranscript
            lines={currentSession.lines}
            pendingPartial={currentSession.pendingPartial}
          />
        </div>
      )}
    </div>
  );
}
