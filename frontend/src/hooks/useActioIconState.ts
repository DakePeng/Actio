import { useVoiceStore } from '../store/use-voice-store';
import type { ActioIconState } from '../components/ActioIcon';

export function useActioIconState(): ActioIconState {
  const isRecording = useVoiceStore((s) => s.isRecording);
  if (isRecording) return 'recording';
  return 'paused';
}
