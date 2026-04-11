import { useEffect, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useVoiceStore } from '../store/use-voice-store';

const MOCK_SENTENCES = [
  'The meeting was productive and all agenda items were covered.',
  'Action items were assigned to each team member.',
  'The deadline has been moved to next Friday.',
  'We need to follow up with the client by end of week.',
  'The new feature request will be added to the backlog.',
  'Budget approval is still pending from finance.',
  'The demo went well and the client was satisfied.',
  'We agreed to reconvene next Tuesday at 10 AM.',
  'The design review is scheduled for Thursday afternoon.',
  'Engineering estimates are due by end of sprint.',
];

function MicIcon() {
  return (
    <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
      <line x1="12" x2="12" y1="19" y2="22" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg width="24" height="24" viewBox="0 0 24 24" fill="currentColor">
      <rect x="6" y="6" width="12" height="12" rx="2" />
    </svg>
  );
}

export function RecordingTab() {
  const isRecording = useVoiceStore((s) => s.isRecording);
  const currentSession = useVoiceStore((s) => s.currentSession);
  const clipInterval = useVoiceStore((s) => s.clipInterval);
  const startRecording = useVoiceStore((s) => s.startRecording);
  const stopRecording = useVoiceStore((s) => s.stopRecording);
  const appendLiveTranscript = useVoiceStore((s) => s.appendLiveTranscript);
  const flushInterval = useVoiceStore((s) => s.flushInterval);

  const [elapsed, setElapsed] = useState(0);
  const transcriptRef = useRef<HTMLDivElement>(null);
  const mockTimerRef = useRef<number | null>(null);
  const clipTimerRef = useRef<number | null>(null);
  const elapsedTimerRef = useRef<number | null>(null);
  const sentenceIndexRef = useRef(0);

  useEffect(() => {
    if (!isRecording) {
      clearAllTimers();
      setElapsed(0);
      return;
    }

    sentenceIndexRef.current = Math.floor(Math.random() * MOCK_SENTENCES.length);

    mockTimerRef.current = window.setInterval(() => {
      const sentence = MOCK_SENTENCES[sentenceIndexRef.current % MOCK_SENTENCES.length];
      sentenceIndexRef.current++;
      appendLiveTranscript(sentence);
    }, 2000);

    clipTimerRef.current = window.setInterval(() => {
      flushInterval();
    }, clipInterval * 60 * 1000);

    elapsedTimerRef.current = window.setInterval(() => {
      setElapsed((prev) => prev + 1);
    }, 1000);

    return clearAllTimers;
  }, [isRecording]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (transcriptRef.current) {
      transcriptRef.current.scrollTop = transcriptRef.current.scrollHeight;
    }
  }, [currentSession?.liveTranscript]);

  function clearAllTimers() {
    if (mockTimerRef.current) window.clearInterval(mockTimerRef.current);
    if (clipTimerRef.current) window.clearInterval(clipTimerRef.current);
    if (elapsedTimerRef.current) window.clearInterval(elapsedTimerRef.current);
    mockTimerRef.current = null;
    clipTimerRef.current = null;
    elapsedTimerRef.current = null;
  }

  const intervalSeconds = clipInterval * 60;
  const secondsIntoInterval = elapsed % intervalSeconds;
  const elapsedMinutes = Math.floor(secondsIntoInterval / 60);
  const elapsedSeconds = secondsIntoInterval % 60;
  const totalMinutes = clipInterval;
  const progress = secondsIntoInterval / intervalSeconds;

  // SVG progress ring
  const RADIUS = 52;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
  const strokeDashoffset = CIRCUMFERENCE * (1 - progress);

  return (
    <div className="recording-tab">
      <div className="recording-tab__controls">
        <div className="recording-tab__btn-wrap">
          {/* Progress ring behind the button */}
          <AnimatePresence>
            {isRecording && (
              <motion.svg
                className="recording-tab__ring"
                width="120"
                height="120"
                viewBox="0 0 120 120"
                initial={{ opacity: 0, scale: 0.8 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.8 }}
                transition={{ type: 'spring', stiffness: 200, damping: 20 }}
              >
                {/* Track */}
                <circle cx="60" cy="60" r={RADIUS} fill="none" stroke="var(--color-border)" strokeWidth="3" />
                {/* Progress */}
                <circle
                  cx="60"
                  cy="60"
                  r={RADIUS}
                  fill="none"
                  stroke="#ef4444"
                  strokeWidth="3"
                  strokeLinecap="round"
                  strokeDasharray={CIRCUMFERENCE}
                  strokeDashoffset={strokeDashoffset}
                  style={{ transform: 'rotate(-90deg)', transformOrigin: '50% 50%', transition: 'stroke-dashoffset 1s linear' }}
                />
              </motion.svg>
            )}
          </AnimatePresence>

          <motion.button
            type="button"
            className={`recording-btn${isRecording ? ' is-recording' : ''}`}
            onClick={isRecording ? stopRecording : startRecording}
            aria-label={isRecording ? 'Stop recording' : 'Start recording'}
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
                  key="mic"
                  initial={{ opacity: 0, scale: 0.5 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.5 }}
                  transition={{ duration: 0.15 }}
                  style={{ display: 'flex' }}
                >
                  <MicIcon />
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
              Tap to record
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
              {String(elapsedMinutes).padStart(2, '0')}:{String(elapsedSeconds).padStart(2, '0')}
              <span className="recording-tab__timer-divider">/</span>
              {String(totalMinutes).padStart(2, '0')}:00
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
            {currentSession.liveTranscript || (
              <span className="recording-tab__transcript-placeholder">Listening...</span>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
