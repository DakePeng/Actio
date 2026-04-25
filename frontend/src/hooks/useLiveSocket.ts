import { useEffect } from 'react';
import { useStore } from '../store/use-store';
import { useVoiceStore } from '../store/use-voice-store';

/** Mirror `ui.listeningEnabled` into the voice store's WS lifecycle.
 *  Mounted once at the app root. Idempotent: startRecording/stopRecording
 *  in use-voice-store both no-op if the WS is already in the target state. */
export function useLiveSocket(): void {
  const listeningEnabled = useStore((s) => s.ui.listeningEnabled);

  useEffect(() => {
    if (listeningEnabled === null) return; // still booting
    if (listeningEnabled) {
      useVoiceStore.getState().startRecording();
    } else {
      useVoiceStore.getState().stopRecording();
    }
  }, [listeningEnabled]);
}
