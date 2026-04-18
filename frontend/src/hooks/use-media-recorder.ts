import { useCallback, useEffect, useRef, useState } from 'react';

function writeString(view: DataView, offset: number, str: string) {
  for (let i = 0; i < str.length; i++) view.setUint8(offset + i, str.charCodeAt(i));
}

/**
 * Encode a Float32Array of 16 kHz mono samples as a 16-bit PCM WAV Blob.
 */
function encodeWav16kMono(samples: Float32Array): Blob {
  const bytesPerSample = 2;
  const headerSize = 44;
  const dataSize = samples.length * bytesPerSample;
  const buffer = new ArrayBuffer(headerSize + dataSize);
  const view = new DataView(buffer);
  // RIFF header
  writeString(view, 0, 'RIFF');
  view.setUint32(4, 36 + dataSize, true);
  writeString(view, 8, 'WAVE');
  // fmt chunk
  writeString(view, 12, 'fmt ');
  view.setUint32(16, 16, true); // PCM chunk size
  view.setUint16(20, 1, true); // PCM format
  view.setUint16(22, 1, true); // channels
  view.setUint32(24, 16000, true); // sample rate
  view.setUint32(28, 16000 * bytesPerSample, true); // byte rate
  view.setUint16(32, bytesPerSample, true); // block align
  view.setUint16(34, 16, true); // bits per sample
  // data chunk
  writeString(view, 36, 'data');
  view.setUint32(40, dataSize, true);
  // samples
  let offset = 44;
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    offset += 2;
  }
  return new Blob([buffer], { type: 'audio/wav' });
}

function linearResample(
  input: Float32Array,
  srcRate: number,
  dstRate: number,
): Float32Array {
  if (srcRate === dstRate) return input;
  const ratio = srcRate / dstRate;
  const outLen = Math.floor(input.length / ratio);
  const out = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const pos = i * ratio;
    const idx = Math.floor(pos);
    const frac = pos - idx;
    const a = input[idx];
    const b = input[idx + 1] ?? a;
    out[i] = a + (b - a) * frac;
  }
  return out;
}

export interface UseMediaRecorder {
  recording: boolean;
  durationSec: number;
  /** 0..1-ish RMS level for a live meter. */
  rmsLevel: number;
  start: () => Promise<void>;
  stop: () => Promise<Blob>;
  cancel: () => void;
  error: string | null;
}

export function useMediaRecorder(): UseMediaRecorder {
  const [recording, setRecording] = useState(false);
  const [durationSec, setDurationSec] = useState(0);
  const [rmsLevel, setRmsLevel] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const ctxRef = useRef<AudioContext | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const chunksRef = useRef<Float32Array[]>([]);
  const processorRef = useRef<ScriptProcessorNode | null>(null);
  const startTsRef = useRef<number>(0);
  const rafRef = useRef<number | null>(null);

  const cleanup = useCallback(() => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    processorRef.current?.disconnect();
    analyserRef.current?.disconnect();
    streamRef.current?.getTracks().forEach((t) => t.stop());
    void ctxRef.current?.close();
    processorRef.current = null;
    analyserRef.current = null;
    streamRef.current = null;
    ctxRef.current = null;
  }, []);

  const start = useCallback(async () => {
    setError(null);
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          channelCount: 1,
          sampleRate: 16000,
          echoCancellation: true,
          noiseSuppression: true,
        },
      });
      streamRef.current = stream;
      const AudioCtx =
        window.AudioContext ||
        (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      const ctx = new AudioCtx({ sampleRate: 16000 });
      ctxRef.current = ctx;
      const source = ctx.createMediaStreamSource(stream);
      const analyser = ctx.createAnalyser();
      analyser.fftSize = 1024;
      analyserRef.current = analyser;
      source.connect(analyser);

      // ScriptProcessorNode is deprecated but works everywhere without a worklet.
      // Buffer size 4096 keeps UI latency ~250ms which is fine for enrollment.
      const processor = ctx.createScriptProcessor(4096, 1, 1);
      processor.onaudioprocess = (e) => {
        const input = e.inputBuffer.getChannelData(0);
        chunksRef.current.push(new Float32Array(input));
      };
      source.connect(processor);
      processor.connect(ctx.destination);
      processorRef.current = processor;

      chunksRef.current = [];
      startTsRef.current = performance.now();
      setRecording(true);

      const buffer = new Float32Array(analyser.fftSize);
      const tick = () => {
        if (!analyserRef.current) return;
        analyserRef.current.getFloatTimeDomainData(buffer);
        let sum = 0;
        for (let i = 0; i < buffer.length; i++) sum += buffer[i] * buffer[i];
        setRmsLevel(Math.sqrt(sum / buffer.length));
        setDurationSec((performance.now() - startTsRef.current) / 1000);
        rafRef.current = requestAnimationFrame(tick);
      };
      tick();
    } catch (err) {
      setError((err as Error).message || 'Microphone access failed');
      cleanup();
      setRecording(false);
      throw err;
    }
  }, [cleanup]);

  const stop = useCallback(async (): Promise<Blob> => {
    if (!ctxRef.current) throw new Error('not recording');
    const srcRate = ctxRef.current.sampleRate;
    const total = chunksRef.current.reduce((n, c) => n + c.length, 0);
    const merged = new Float32Array(total);
    let o = 0;
    for (const c of chunksRef.current) {
      merged.set(c, o);
      o += c.length;
    }
    const resampled = linearResample(merged, srcRate, 16000);
    const blob = encodeWav16kMono(resampled);
    cleanup();
    setRecording(false);
    return blob;
  }, [cleanup]);

  const cancel = useCallback(() => {
    chunksRef.current = [];
    cleanup();
    setRecording(false);
  }, [cleanup]);

  useEffect(() => () => cleanup(), [cleanup]);

  return { recording, durationSec, rmsLevel, start, stop, cancel, error };
}
