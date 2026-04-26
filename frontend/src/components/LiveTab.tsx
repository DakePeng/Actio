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

/** Five-bar audio-level visualisation. We don't have backend RMS on
 *  the WebSocket today, so the wave reflects PROXY activity: a fresh
 *  transcript or partial flips it to high amplitude for ~600ms; in
 *  between, a gentle baseline animation when listening, flat when
 *  muted. Replace with real RMS once the backend broadcasts it. */
function VoiceWave() {
  const isListening = useStore((s) => s.ui.listeningEnabled);
  const partialText = useVoiceStore((s) => s.currentSession?.pendingPartial?.text ?? '');
  const linesLen = useVoiceStore((s) => s.currentSession?.lines.length ?? 0);
  const [active, setActive] = useState(false);

  useEffect(() => {
    if (!isListening) return;
    setActive(true);
    const id = window.setTimeout(() => setActive(false), 600);
    return () => window.clearTimeout(id);
  }, [partialText, linesLen, isListening]);

  const stateClass = !isListening
    ? 'voice-wave--idle'
    : active
      ? 'voice-wave--active'
      : 'voice-wave--quiet';

  return (
    <div className={`voice-wave ${stateClass}`} aria-hidden="true">
      {[0, 1, 2, 3, 4].map((i) => (
        <span
          key={i}
          className="voice-wave__bar"
          style={{ animationDelay: `${i * 80}ms` }}
        />
      ))}
    </div>
  );
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
      <header className="live-tab__topbar">
        <div className="live-tab__topbar-left">
          <span className={`live-tab__status${isOn ? ' is-on' : ''}`}>
            {headerLabel}
          </span>
          {isOn && listeningStartedAt && (
            <p className="live-tab__since" aria-live="polite">
              {t('live.listeningSince', {
                time: formatTime(listeningStartedAt),
                duration: formatDuration(now - listeningStartedAt),
              })}
            </p>
          )}
        </div>
      </header>

      <main className="live-tab__main" ref={transcriptRef} aria-live="polite">
        {isOn && currentSession ? (
          <LiveTranscript
            lines={currentSession.lines}
            pendingPartial={currentSession.pendingPartial}
          />
        ) : (
          <div className="live-tab__empty">
            <p className="live-tab__empty-body">
              {!isOn ? t('live.pausedHint') : t('recording.startingUp')}
            </p>
          </div>
        )}
      </main>

      <footer className="live-tab__footer">
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
        <VoiceWave />
        <ListeningToggle size={48} iconSize={22} />
      </footer>
    </div>
  );
}
