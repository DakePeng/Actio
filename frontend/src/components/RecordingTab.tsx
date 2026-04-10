import { useEffect, useRef, useState } from 'react';
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

  return (
    <div className="recording-tab">
      <div className="recording-tab__controls">
        <button
          type="button"
          className={`recording-btn${isRecording ? ' is-recording' : ''}`}
          onClick={isRecording ? stopRecording : startRecording}
          aria-label={isRecording ? 'Stop recording' : 'Start recording'}
        >
          {isRecording ? '⏹' : '🎙'}
        </button>
        {!isRecording && <p className="recording-tab__hint">Tap to record</p>}
        {isRecording && (
          <p className="recording-tab__timer" aria-live="polite">
            {String(elapsedMinutes).padStart(2, '0')}:{String(elapsedSeconds).padStart(2, '0')}
            {' / '}
            {String(totalMinutes).padStart(2, '0')}:00
          </p>
        )}
      </div>
      {isRecording && currentSession && (
        <div className="recording-tab__transcript" ref={transcriptRef} aria-live="polite">
          {currentSession.liveTranscript || (
            <span className="recording-tab__transcript-placeholder">Listening…</span>
          )}
        </div>
      )}
    </div>
  );
}
