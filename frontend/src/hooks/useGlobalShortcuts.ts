import { useEffect, useRef } from 'react';
import { useStore } from '../store/use-store';
import { getApiUrl, getWsUrl } from '../api/backend-url';
import { primaryMod } from '../utils/platform';
import { flashWordmark } from './useWordmarkFlash';

// `@tauri-apps/api` is imported dynamically so the Tauri runtime surface
// code-splits out of the main bundle (see ISSUES.md #51). The module-scope
// promise cache means each submodule resolves at most once across the whole
// hook — multiple useEffects that need `listen` or `invoke` await the same
// resolved namespace, which avoids repeated `import()` calls confusing
// vitest's mock layer (it has trouble re-applying mocks to repeated dynamic
// imports of the same module).
let eventP: Promise<typeof import('@tauri-apps/api/event')> | null = null;
let coreP: Promise<typeof import('@tauri-apps/api/core')> | null = null;
const loadEvent = () => (eventP ??= import('@tauri-apps/api/event'));
const loadCore = () => (coreP ??= import('@tauri-apps/api/core'));

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// Defaults applied on first launch only when the backend has no persisted
// shortcut for an action. `primaryMod` resolves to "Super" on macOS (which
// tauri-plugin-global-shortcut binds to the Cmd key) and "Ctrl" elsewhere.
const DEFAULT_GLOBAL_SHORTCUTS: Record<string, string> = {
  toggle_board_tray: `${primaryMod}+\\`,
  start_dictation: `${primaryMod}+Shift+Space`,
  new_todo: `${primaryMod}+N`,
  toggle_listening: `${primaryMod}+Shift+M`,
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

  /** Paste accumulated transcript, close WS, clear transcribing state.
   *  Async because `invoke` is dynamically imported (see ISSUES.md #51);
   *  callers don't await — fire-and-forget is the desired semantics. */
  async function finishDictation() {
    if (transcribingTimerRef.current) {
      window.clearTimeout(transcribingTimerRef.current);
      transcribingTimerRef.current = null;
    }
    const transcript = fullTranscriptRef.current;
    if (transcript) {
      console.log('[Actio] Pasting transcript:', transcript);
      const { invoke } = await loadCore();
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

  // Register global shortcuts on mount, preferring whatever the backend has
  // persisted (the user's customizations) over JS defaults. Earlier versions
  // of this effect blindly re-registered DEFAULT_GLOBAL_SHORTCUTS on every
  // mount, which on macOS clobbered platform-aware defaults from the backend.
  useEffect(() => {
    if (!isTauri) return;
    let cancelled = false;

    (async () => {
      let shortcuts: Record<string, string> = { ...DEFAULT_GLOBAL_SHORTCUTS };
      try {
        const res = await fetch(await getApiUrl('/settings'));
        if (res.ok) {
          const data = await res.json();
          const persisted: Record<string, string> | undefined =
            data?.keyboard?.shortcuts;
          if (persisted) {
            // Only merge keys we know about as global actions; ignore
            // anything else the backend may have stored.
            for (const action of Object.keys(DEFAULT_GLOBAL_SHORTCUTS)) {
              if (typeof persisted[action] === 'string' && persisted[action]) {
                shortcuts[action] = persisted[action];
              }
            }
          }
        }
      } catch (e) {
        console.warn('[Actio] Could not fetch persisted shortcuts, using defaults:', e);
      }

      if (cancelled) return;
      try {
        const { invoke } = await loadCore();
        await invoke('reregister_shortcuts', { shortcuts });
        console.log('[Actio] Global shortcuts registered:', shortcuts);
      } catch (e) {
        console.error('[Actio] Failed to register global shortcuts:', e);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  // Listen for shortcut-triggered events
  useEffect(() => {
    if (!isTauri) return;

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    (async () => {
      const { listen } = await loadEvent();
      if (cancelled) return;

      const fn = await listen<string>('shortcut-triggered', async (event) => {
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
          const { invoke } = await loadCore();
          const { isDictating, isDictationTranscribing } = useStore.getState().ui;
          if (isDictating || isDictationTranscribing) {
            console.log('[Actio] Stopping dictation...');
            invoke('stop_dictation').catch(console.error);
            // If we already have a transcript, paste immediately
            if (fullTranscriptRef.current) {
              console.log('[Actio] Transcript ready, pasting now');
              void finishDictation();
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
                void finishDictation();
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
      });

      if (cancelled) { fn(); return; }
      unlisten = fn;
    })();

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

    (async () => {
      const { listen } = await loadEvent();
      if (cancelled) return;

      const fn = await listen<string>('dictation-status', async (e) => {
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

          // Resolve via getWsUrl so port-fallback (3000-3009) and the eventual
          // production WebView origin both work — hardcoding ws://127.0.0.1:3000
          // breaks dictation when another process holds 3000.
          let wsUrl: string;
          try {
            wsUrl = await getWsUrl('/ws');
          } catch (err) {
            console.error('[Actio] Could not resolve WS URL, dictation aborted:', err);
            return;
          }
          if (cancelled) return;
          const ws = new WebSocket(wsUrl);
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
                  void finishDictation();
                }
              }
            } catch { /* ignore non-JSON messages like pings */ }
          };
          ws.onerror = (err) => console.error('[Actio] Dictation WS error:', err);
        }
        // Note: we don't close WS on 'idle' status; the shortcut handler
        // manages the transition to transcribing phase, which keeps WS open.
      });

      if (cancelled) { fn(); return; }
      unlisten = fn;
    })();

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
