"""VAD gRPC service — detects speech segments in audio chunks."""
import logging
import struct

import grpc

import inference_pb2
import inference_pb2_grpc

logger = logging.getLogger("actio-worker.vad")


class VADService(inference_pb2_grpc.VADServiceServicer):
    """VAD service that detects speech in audio streams.

    MVP implementation: energy-based VAD (simple RMS threshold).
    Future: integrate Silero VAD or FunASR VAD model.
    """

    SPEECH_THRESHOLD = 0.01  # RMS threshold for speech detection
    MIN_SEGMENT_MS = 300     # Minimum segment duration

    def DetectSpeech(self, request_iterator, context):
        """Process streaming audio chunks and return VAD results."""
        segment_start_ms = None
        session_id = ""

        for chunk in request_iterator:
            audio_data = chunk.audio_data
            session_id = chunk.session_id
            timestamp_ms = chunk.timestamp_ms

            is_speech = self._is_speech(audio_data)

            if is_speech and segment_start_ms is None:
                segment_start_ms = timestamp_ms
            elif not is_speech and segment_start_ms is not None:
                duration = timestamp_ms - segment_start_ms
                if duration >= self.MIN_SEGMENT_MS:
                    yield inference_pb2.VADResult(
                        is_speech=True,
                        segment_start_ms=segment_start_ms,
                        segment_end_ms=timestamp_ms,
                        confidence=0.8,
                        session_id=session_id,
                    )
                segment_start_ms = None

        # Flush final segment if still active
        if segment_start_ms is not None:
            yield inference_pb2.VADResult(
                is_speech=True,
                segment_start_ms=segment_start_ms,
                segment_end_ms=segment_start_ms + self.MIN_SEGMENT_MS,
                confidence=0.7,
                session_id=session_id,
            )

    @staticmethod
    def _is_speech(audio_data: bytes) -> bool:
        """Simple energy-based VAD using RMS of 16-bit PCM samples."""
        if len(audio_data) < 2:
            return False

        num_samples = len(audio_data) // 2
        samples = struct.unpack(f'<{num_samples}h', audio_data[:num_samples * 2])

        if not samples:
            return False

        # RMS energy
        rms = (sum(s * s for s in samples) / len(samples)) ** 0.5
        # Normalize to [-1, 1] range (16-bit max = 32768)
        normalized = rms / 32768.0

        return normalized > 0.01
