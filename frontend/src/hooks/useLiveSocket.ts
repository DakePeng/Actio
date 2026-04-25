import { useEffect } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';

const TRANSLATE_BATCH_INTERVAL_MS = 3000;

/** Mirror `ui.listeningEnabled` into the voice store's WS lifecycle,
 *  AND drive a 3-second translation flush while listening + translation
 *  are both on. Mounted once at the app root. */
export function useLiveSocket(): void {
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);
  const translateEnabled = useVoiceStore((s) => s.translation.enabled);
  const hasSession = useVoiceStore((s) => s.currentSession !== null);

  // WS lifecycle.
  useEffect(() => {
    if (listeningEnabled === null) return;
    if (listeningEnabled) {
      useVoiceStore.getState().startRecording();
    } else {
      useVoiceStore.getState().stopRecording();
    }
  }, [listeningEnabled]);

  // Translation flush loop.
  useEffect(() => {
    if (!translateEnabled || !hasSession) return;
    const id = window.setInterval(() => {
      void useVoiceStore.getState().flushTranslationBatch();
    }, TRANSLATE_BATCH_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [translateEnabled, hasSession]);
}
