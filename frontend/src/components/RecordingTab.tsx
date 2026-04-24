import { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useVoiceStore } from '../store/use-voice-store';
import { getApiUrl } from '../api/backend-url';
import { LiveTranscript } from './LiveTranscript';
import { useT } from '../i18n';

type WarmupState = 'idle' | 'warming' | 'ready' | 'error';

/** Five vertical bars. When `animated` is true, each bar oscillates in height
 *  with a staggered delay — looks like a live audio meter. */
function WaveformIcon({ animated }: { animated?: boolean }) {
  // Base (resting) y1/y2 per bar — a peaked shape rising to the middle.
  const bars = [
    { x: 4, y1: 10, y2: 14, amp: 2 },
    { x: 8, y1: 7, y2: 17, amp: 3 },
    { x: 12, y1: 4, y2: 20, amp: 4 },
    { x: 16, y1: 7, y2: 17, amp: 3 },
    { x: 20, y1: 10, y2: 14, amp: 2 },
  ];

  return (
    <svg
      width="30"
      height="30"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {bars.map((b, i) =>
        animated ? (
          <motion.line
            key={i}
            x1={b.x}
            x2={b.x}
            initial={{ y1: b.y1, y2: b.y2 }}
            animate={{
              y1: [b.y1, Math.max(2, b.y1 - b.amp), b.y1 + b.amp, b.y1],
              y2: [b.y2, Math.min(22, b.y2 + b.amp), b.y2 - b.amp, b.y2],
            }}
            transition={{
              duration: 1.1,
              repeat: Infinity,
              ease: 'easeInOut',
              delay: i * 0.12,
            }}
          />
        ) : (
          <line key={i} x1={b.x} x2={b.x} y1={b.y1} y2={b.y2} />
        ),
      )}
    </svg>
  );
}

function StopIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </svg>
  );
}

/** Three dots that pulse with staggered delays — used next to "Loading model". */
function AnimatedDots() {
  return (
    <span className="loading-dots" aria-hidden="true">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="loading-dots__dot"
          animate={{ opacity: [0.2, 1, 0.2] }}
          transition={{
            duration: 1.1,
            repeat: Infinity,
            ease: 'easeInOut',
            delay: i * 0.18,
          }}
        />
      ))}
    </span>
  );
}

function formatElapsed(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

export function RecordingTab() {
  const isRecording = useVoiceStore((s) => s.isRecording);
  const currentSession = useVoiceStore((s) => s.currentSession);
  const startRecording = useVoiceStore((s) => s.startRecording);
  const stopRecording = useVoiceStore((s) => s.stopRecording);
  const t = useT();

  const [elapsed, setElapsed] = useState(0);
  const [warmupState, setWarmupState] = useState<WarmupState>('idle');
  const transcriptRef = useRef<HTMLDivElement>(null);
  const elapsedTimerRef = useRef<number | null>(null);

  // True once the backend has confirmed the file-cache warmup finished.
  // Independent of whether the user has pressed record yet.
  const warming = warmupState === 'warming';

  // True while the user has pressed record but the pipeline hasn't yet
  // produced any transcript output. Separate from warmup.
  const pipelineStarting = isRecording && currentSession !== null && !currentSession.pipelineReady;

  // Preload the ASR model when the Transcribe tab mounts so the next
  // start-recording click doesn't pay the cold-read penalty on model files.
  // The backend warmup endpoint now blocks until files are resident in the
  // OS page cache, so `ready` corresponds to real readiness.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        setWarmupState('warming');
        const settingsRes = await fetch(await getApiUrl('/settings'));
        if (!settingsRes.ok) throw new Error(`settings HTTP ${settingsRes.status}`);
        const settings = await settingsRes.json();
        const asrModel: string | undefined = settings.audio?.asr_model;
        if (!asrModel) {
          if (!cancelled) setWarmupState('ready');
          return;
        }
        const warmRes = await fetch(await getApiUrl('/settings/models/warmup'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ asr_model: asrModel }),
        });
        if (!warmRes.ok) throw new Error(`warmup HTTP ${warmRes.status}`);
        if (!cancelled) setWarmupState('ready');
      } catch (e) {
        console.warn('[Actio] Warmup failed', e);
        if (!cancelled) setWarmupState('error');
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Elapsed-time ticker — no clip interval, no upper bound.
  useEffect(() => {
    if (!isRecording) {
      if (elapsedTimerRef.current) window.clearInterval(elapsedTimerRef.current);
      elapsedTimerRef.current = null;
      setElapsed(0);
      return;
    }

    elapsedTimerRef.current = window.setInterval(() => {
      setElapsed((prev) => prev + 1);
    }, 1000);

    return () => {
      if (elapsedTimerRef.current) window.clearInterval(elapsedTimerRef.current);
      elapsedTimerRef.current = null;
    };
  }, [isRecording]);

  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [currentSession?.lines, currentSession?.pendingPartial]);

  const hintContent = (() => {
    if (warming) {
      return (
        <>
          {t('recording.loadingModel')}
          <AnimatedDots />
        </>
      );
    }
    if (warmupState === 'error') return t('recording.modelLoadFailed');
    return t('recording.tapToTranscribe');
  })();

  return (
    <div className="recording-tab">
      <div className="recording-tab__controls">
        <div className="recording-tab__btn-wrap">
          {/* Pulsing halo ring during warmup */}
          <AnimatePresence>
            {warming && (
              <>
                <motion.span
                  key="halo-1"
                  className="recording-btn__halo"
                  initial={{ opacity: 0, scale: 0.9 }}
                  animate={{ opacity: [0, 0.55, 0], scale: [0.9, 1.45, 1.55] }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 1.8, repeat: Infinity, ease: 'easeOut' }}
                />
                <motion.span
                  key="halo-2"
                  className="recording-btn__halo"
                  initial={{ opacity: 0, scale: 0.9 }}
                  animate={{ opacity: [0, 0.35, 0], scale: [0.9, 1.25, 1.35] }}
                  exit={{ opacity: 0 }}
                  transition={{
                    duration: 1.8,
                    repeat: Infinity,
                    ease: 'easeOut',
                    delay: 0.6,
                  }}
                />
              </>
            )}
          </AnimatePresence>

          <motion.button
            type="button"
            className={`recording-btn${isRecording ? ' is-recording' : ''}${warming ? ' is-warming' : ''}`}
            onClick={isRecording ? stopRecording : startRecording}
            aria-label={
              isRecording
                ? t('recording.aria.stopTranscribing')
                : t('recording.aria.startTranscribing')
            }
            aria-busy={warming || pipelineStarting}
            whileHover={{ scale: 1.06 }}
            whileTap={{ scale: 0.95 }}
            layout
          >
            <AnimatePresence mode="wait">
              {isRecording ? (
                <motion.span
                  key="stop"
                  initial={{ opacity: 0, scale: 0.5 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.5 }}
                  transition={{ duration: 0.15 }}
                  style={{ display: 'flex' }}
                >
                  <StopIcon />
                </motion.span>
              ) : (
                <motion.span
                  key="wave"
                  initial={{ opacity: 0, scale: 0.5 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.5 }}
                  transition={{ duration: 0.15 }}
                  style={{ display: 'flex' }}
                >
                  <WaveformIcon animated={warming} />
                </motion.span>
              )}
            </AnimatePresence>
          </motion.button>
        </div>

        <AnimatePresence mode="wait">
          {!isRecording ? (
            <motion.p
              key="hint"
              className="recording-tab__hint"
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -8 }}
              transition={{ duration: 0.2 }}
            >
              {hintContent}
            </motion.p>
          ) : (
            <motion.p
              key="timer"
              className="recording-tab__timer"
              initial={{ opacity: 0, y: 8 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -8 }}
              transition={{ duration: 0.2 }}
              aria-live="polite"
            >
              {formatElapsed(elapsed)}
            </motion.p>
          )}
        </AnimatePresence>
      </div>

      <AnimatePresence>
        {isRecording && currentSession && (
          <motion.div
            className="recording-tab__transcript"
            ref={transcriptRef}
            initial={{ opacity: 0, y: 20, height: 0 }}
            animate={{ opacity: 1, y: 0, height: 'auto' }}
            exit={{ opacity: 0, y: 20, height: 0 }}
            transition={{ type: 'spring', stiffness: 200, damping: 22 }}
            aria-live="polite"
          >
            {pipelineStarting &&
            currentSession.lines.length === 0 &&
            !currentSession.pendingPartial ? (
              <span className="recording-tab__starting">
                {t('recording.startingUp')}
                <AnimatedDots />
              </span>
            ) : (
              <LiveTranscript
                lines={currentSession.lines}
                pendingPartial={currentSession.pendingPartial}
              />
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
