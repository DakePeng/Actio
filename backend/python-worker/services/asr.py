"""ASR gRPC service — streaming speech recognition using FunASR."""
import logging

import grpc
import numpy as np

import inference_pb2
import inference_pb2_grpc

logger = logging.getLogger("actio-worker.asr")


class ASRService(inference_pb2_grpc.ASRServiceServicer):
    """ASR service using FunASR Paraformer-Streaming for real-time recognition."""

    def __init__(self, model_loader):
        self._model_loader = model_loader

    def StreamRecognize(self, request_iterator, context):
        """Bidirectional streaming ASR — receive audio chunks, return transcripts."""
        try:
            model = self._model_loader.get_asr_model()
        except Exception as e:
            logger.error(f"Cannot load ASR model: {e}")
            context.set_code(grpc.StatusCode.UNAVAILABLE)
            context.set_details(f"ASR model not available: {e}")
            return

        chunk_buffer = bytearray()
        chunk_count = 0
        session_id = ""

        for chunk in request_iterator:
            chunk_buffer.extend(chunk.audio_data)
            chunk_count += 1
            session_id = chunk.session_id

            # Process every ~2 chunks (~1.2s of audio) for partial results
            if chunk_count % 2 == 0 and len(chunk_buffer) > 0:
                try:
                    # Convert bytes to numpy array (16-bit PCM -> float32)
                    audio_np = np.frombuffer(bytes(chunk_buffer), dtype=np.int16).astype(np.float32) / 32768.0

                    result = model.generate(
                        input=audio_np,
                        batch_size_s=300,
                    )

                    if result and result[0].get("text"):
                        text = result[0]["text"]
                        is_final = chunk_count % 10 == 0  # Periodic final

                        yield inference_pb2.RecognizeResult(
                            text=text,
                            is_final=is_final,
                            start_ms=0,
                            end_ms=chunk_count * 600,
                            session_id=session_id,
                        )

                        if is_final:
                            chunk_buffer.clear()

                except Exception as e:
                    logger.error(f"ASR inference error: {e}")

    def RecognizeFile(self, request, context):
        """Unary file recognition — process entire audio at once."""
        try:
            model = self._model_loader.get_asr_model()
        except Exception as e:
            context.set_code(grpc.StatusCode.UNAVAILABLE)
            context.set_details(f"ASR model not available: {e}")
            return inference_pb2.RecognizeFileResponse()

        audio_np = np.frombuffer(request.audio_data, dtype=np.int16).astype(np.float32) / 32768.0

        try:
            result = model.generate(input=audio_np, batch_size_s=300)
            segments = []
            if result and result[0].get("text"):
                segments.append(inference_pb2.TranscriptSegment(
                    text=result[0]["text"],
                    start_ms=0,
                    end_ms=int(len(audio_np) / 16),  # Approximate
                    is_final=True,
                ))

            return inference_pb2.RecognizeFileResponse(segments=segments)

        except Exception as e:
            logger.error(f"File recognition error: {e}")
            return inference_pb2.RecognizeFileResponse()
