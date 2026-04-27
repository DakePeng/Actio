import { useEffect, useRef, useState, type CSSProperties } from 'react';
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

/** Five-bar audio-level visualisation driven by real mic RMS streamed
 *  from the backend at ~15Hz. Each bar maps the raw RMS through a
 *  per-bar amplitude multiplier so the bars never move in lockstep —
 *  natural-looking "wave" instead of a single rising/falling block.
 *
 *  Gain mapping: typical speech RMS sits in 0.02–0.20; we normalise
 *  via `min(1, rms * 6)` so an RMS of 0.17 saturates the bars. */
const BAR_AMPS = [0.55, 0.85, 1.0, 0.78, 0.5] as const;

function VoiceWave() {
  const isListening = useStore((s) => s.ui.listeningEnabled);
  const audioLevel = useVoiceStore((s) => s.audioLevel);

  const gain = isListening ? Math.min(1, audioLevel * 6) : 0;
  const stateClass = !isListening ? 'voice-wave--idle' : 'voice-wave--live';

  return (
    <div className={`voice-wave ${stateClass}`} aria-hidden="true">
      {BAR_AMPS.map((amp, i) => (
        <span
          key={i}
          className="voice-wave__bar"
          style={{ '--bar-h': `${4 + gain * amp * 26}px` } as CSSProperties}
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
  // Tracks whether the user is currently "following live" — i.e. their
  // scroll position is at (or within FOLLOW_THRESHOLD_PX of) the bottom of
  // the transcript. Defaults to true so the very first lines auto-scroll.
  // Once they scroll up to read older content, this flips to false and
  // new lines/partials no longer yank them down. It flips back to true the
  // moment they scroll back to within the threshold. See ISSUES.md #57.
  const wasAtBottomRef = useRef(true);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!listeningEnabled || !listeningStartedAt) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [listeningEnabled, listeningStartedAt]);

  useEffect(() => {
    const el = transcriptRef.current;
    if (el && wasAtBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [currentSession?.lines, currentSession?.pendingPartial]);

  /** Threshold (px) within which the user is treated as "at the bottom"
   *  for follow-live purposes. Picked so a single line of accidental
   *  inertial overshoot still counts as following, but any deliberate
   *  scroll-up to read older content (which usually moves the viewport
   *  by more than one bubble) flips out of follow mode. */
  const FOLLOW_THRESHOLD_PX = 64;
  const handleTranscriptScroll = (e: React.UIEvent<HTMLElement>) => {
    const el = e.currentTarget;
    wasAtBottomRef.current =
      el.scrollHeight - el.scrollTop - el.clientHeight < FOLLOW_THRESHOLD_PX;
  };

  const isOn = listeningEnabled === true;
  const headerLabel = isOn ? t('live.header.on') : t('live.header.off');

  // One-shot ARIA status that announces only the on/off transitions —
  // crucially NOT the per-second duration tick. Without this split, the
  // pill below carries `aria-live` and assistive tech announces the
  // elapsed-time string ~3,600× per hour (ISSUES.md #79).
  // Empty on initial mount so the page-load state ("Muted") doesn't
  // announce as if the user just stopped a session.
  const [transitionMessage, setTransitionMessage] = useState('');
  const prevIsOnRef = useRef(isOn);
  useEffect(() => {
    if (prevIsOnRef.current === isOn) return;
    prevIsOnRef.current = isOn;
    if (isOn && listeningStartedAt) {
      setTransitionMessage(
        t('live.aria.listeningStarted', { time: formatTime(listeningStartedAt) }),
      );
    } else if (!isOn) {
      setTransitionMessage(t('live.aria.listeningStopped'));
    }
  }, [isOn, listeningStartedAt, t]);

  return (
    <div className="live-tab">
      <span className="visually-hidden" role="status" aria-live="polite">
        {transitionMessage}
      </span>
      <header className="live-tab__topbar">
        <div className="live-tab__topbar-left">
          <span className={`live-tab__status${isOn ? ' is-on' : ''}`}>
            {headerLabel}
          </span>
          {isOn && listeningStartedAt && (
            <p className="live-tab__since">
              {t('live.listeningSince', {
                time: formatTime(listeningStartedAt),
                duration: formatDuration(now - listeningStartedAt),
              })}
            </p>
          )}
        </div>
      </header>

      <main
        className="live-tab__main"
        ref={transcriptRef}
        aria-live="polite"
        onScroll={handleTranscriptScroll}
      >
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
