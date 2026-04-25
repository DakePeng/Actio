import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useStore } from '../store/use-store';
import { flashWordmark } from './useWordmarkFlash';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const DEFAULT_GLOBAL_SHORTCUTS: Record<string, string> = {
  toggle_board_tray: 'Ctrl+\\',
  start_dictation: 'Ctrl+Shift+Space',
  new_todo: 'Ctrl+N',
  toggle_listening: 'Ctrl+Shift+M',
};

/** How long to wait for a final transcript after stopping dictation (ms).
 *  Used when at least one partial arrived during capture and we expect the
 *  ASR to deliver a final shortly. */
const TRANSCRIBING_TIMEOUT = 5000;

/** Tail wait when no transcript chunks arrived during capture. The
 *  obvious case is silence, but it also covers the (more common) case
 *  where the user speaks a quick word and stops the hotkey before the
 *  ASR has had time to emit even a partial — streaming Zipformer's
 *  first-partial latency is ~300–500ms, and a final after end-of-speech
 *  lands ~500–1000ms later. We need to wait long enough for that round
 *  trip, while still being noticeably faster than the full 5s timeout
 *  used when partials are already streaming. */
const NO_SPEECH_TAIL_TIMEOUT = 2000;

export function useGlobalShortcuts() {
  const setBoardWindow = useStore((s) => s.setBoardWindow);
  const setNewReminderBar = useStore((s) => s.setNewReminderBar);
  const setDictating = useStore((s) => s.setDictating);
  const setDictationTranscript = useStore((s) => s.setDictationTranscript);
  const wsRef = useRef<WebSocket | null>(null);
  const fullTranscriptRef = useRef('');
  const transcribingTimerRef = useRef<number | null>(null);
  // Tracks whether we saw any transcript message at all (partial or final)
  // during the current dictation. If we didn't, there's nothing in flight
  // to wait for at stop time and we can short-circuit the tail timeout.
  const receivedAnyTranscriptRef = useRef(false);

  /** Paste accumulated transcript, close WS, clear transcribing state */
  function finishDictation() {
    if (transcribingTimerRef.current) {
      window.clearTimeout(transcribingTimerRef.current);
      transcribingTimerRef.current = null;
    }
    const transcript = fullTranscriptRef.current;
    if (transcript) {
      console.log('[Actio] Pasting transcript:', transcript);
      invoke('paste_text', { text: transcript }).catch(console.error);
      // Brief 'success' pulse on the wordmark to mark the paste; after it
      // expires the wordmark falls back to the live state (listening if the
      // background pipeline is on, standby otherwise).
      flashWordmark('success', 1200);
    }
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    useStore.setState((s) => ({
      ui: { ...s.ui, isDictating: false, isDictationTranscribing: false, dictationTranscript: '' },
    }));
  }

  // Register default global shortcuts on mount
  useEffect(() => {
    if (!isTauri) return;
    console.log('[Actio] Registering global shortcuts...');
    invoke('reregister_shortcuts', { shortcuts: DEFAULT_GLOBAL_SHORTCUTS })
      .then(() => console.log('[Actio] Global shortcuts registered'))
      .catch((e) => console.error('[Actio] Failed to register global shortcuts:', e));
  }, []);

  // Listen for shortcut-triggered events
  useEffect(() => {
    if (!isTauri) return;

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    listen<string>('shortcut-triggered', async (event) => {
      if (cancelled) return;
      const action = event.payload;
      console.log('[Actio] shortcut-triggered:', action, 'showNewReminderBar:', useStore.getState().ui.showNewReminderBar);

      if (action === 'toggle_board_tray') {
        const current = useStore.getState().ui.showBoardWindow;
        setBoardWindow(!current);
      } else if (action === 'new_todo') {
        const current = useStore.getState().ui.showNewReminderBar;
        setNewReminderBar(!current);
      } else if (action === 'start_dictation') {
        // If the new-item window is open, route dictation to the composer's
        // mic button instead of the global paste pipeline.
        if (useStore.getState().ui.showNewReminderBar) {
          console.log('[Actio] Routing dictation to composer mic');
          window.dispatchEvent(new CustomEvent('actio-toggle-composer-dictation'));
          return;
        }
        const { isDictating, isDictationTranscribing } = useStore.getState().ui;
        if (isDictating || isDictationTranscribing) {
          console.log('[Actio] Stopping dictation...');
          invoke('stop_dictation').catch(console.error);
          // If we already have a transcript, paste immediately
          if (fullTranscriptRef.current) {
            console.log('[Actio] Transcript ready, pasting now');
            finishDictation();
          } else {
            // Enter "transcribing" phase; keep WS open to catch final result.
            useStore.setState((s) => ({
              ui: { ...s.ui, isDictating: false, isDictationTranscribing: true },
            }));
            // If we never received any transcript during capture, there's
            // nothing in flight — use a near-instant tail so the wordmark
            // doesn't dwell on processing for 5s of silence.
            const timeoutMs = receivedAnyTranscriptRef.current
              ? TRANSCRIBING_TIMEOUT
              : NO_SPEECH_TAIL_TIMEOUT;
            transcribingTimerRef.current = window.setTimeout(() => {
              console.log('[Actio] Transcribing timeout, pasting what we have');
              finishDictation();
            }, timeoutMs);
          }
        } else {
          console.log('[Actio] Starting dictation...');
          invoke('start_dictation').catch(console.error);
        }
      } else if (action === 'toggle_listening') {
        const current = useStore.getState().ui.listeningEnabled;
        if (current === null) return;
        const next = !current;
        await useStore.getState().setListening(next);
        // setListening reverts + pushes its own failure toast on PATCH failure;
        // only emit the success toast if the post-call state matches what we
        // optimistically tried to apply (i.e., no rollback occurred).
        if (useStore.getState().ui.listeningEnabled === next) {
          useStore.getState().setFeedback(
            next ? 'feedback.listeningOn' : 'feedback.listeningOff',
            'success',
          );
        }
      }
    }).then((fn) => {
      if (cancelled) { fn(); return; }
      unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setBoardWindow, setNewReminderBar, setDictating, setDictationTranscript]);

  // Listen for dictation-status events to sync store + manage WS connection
  useEffect(() => {
    if (!isTauri) return;

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    listen<string>('dictation-status', (e) => {
      if (cancelled) return;
      const isListening = e.payload === 'listening';
      console.log('[Actio] dictation-status:', e.payload);

      if (isListening) {
        // Close any prior WS
        if (wsRef.current) {
          wsRef.current.close();
          wsRef.current = null;
        }
        setDictating(true);
        setDictationTranscript('');
        fullTranscriptRef.current = '';
        receivedAnyTranscriptRef.current = false;

        const ws = new WebSocket('ws://127.0.0.1:3000/ws');
        wsRef.current = ws;

        const finalizedIds = new Set<string>();
        ws.onmessage = (msg) => {
          try {
            const data = JSON.parse(msg.data);
            if (data.kind === 'transcript' && data.text) {
              if (data.transcript_id && data.is_final && finalizedIds.has(data.transcript_id)) return;
              if (data.transcript_id && data.is_final) finalizedIds.add(data.transcript_id);

              receivedAnyTranscriptRef.current = true;
              console.log('[Actio] Live transcript:', data.text, data.is_final ? '(final)' : '(partial)', 'id:', data.transcript_id);
              if (data.is_final) {
                fullTranscriptRef.current += data.text;
              }
              setDictationTranscript(data.is_final ? fullTranscriptRef.current : `${fullTranscriptRef.current}${data.text}`);

              // If we're in "transcribing" phase and got a final, auto-paste
              if (data.is_final && useStore.getState().ui.isDictationTranscribing) {
                finishDictation();
              }
            }
          } catch { /* ignore non-JSON messages like pings */ }
        };
        ws.onerror = (err) => console.error('[Actio] Dictation WS error:', err);
      }
      // Note: we don't close WS on 'idle' status; the shortcut handler
      // manages the transition to transcribing phase, which keeps WS open.
    }).then((fn) => {
      if (cancelled) { fn(); return; }
      unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, [setDictating, setDictationTranscript]);
}
