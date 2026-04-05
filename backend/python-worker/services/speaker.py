"""Speaker gRPC service — embedding extraction and verification using CAM++."""
import logging

import grpc
import numpy as np

import inference_pb2
import inference_pb2_grpc

logger = logging.getLogger("actio-worker.speaker")


class SpeakerService(inference_pb2_grpc.SpeakerServiceServicer):
    """Speaker service using CAM++ for 192-dim embedding extraction."""

    def __init__(self, model_loader):
        self._model_loader = model_loader

    def _extract_embedding_internal(self, request):
        """Internal embedding extraction (no gRPC context interference)."""
        audio_data = request.audio_data
        sample_rate = int(request.sample_rate) if request.sample_rate > 0 else 16000

        try:
            model = self._model_loader.get_speaker_model()
        except Exception as e:
            logger.error(f"Cannot load speaker model: {e}")
            return inference_pb2.EmbeddingResponse()

        # Convert to numpy float32
        audio_np = np.frombuffer(audio_data, dtype=np.int16).astype(np.float32) / 32768.0

        duration_ms = len(audio_np) / sample_rate * 1000

        # CAM++ needs at least 2s of audio for stable embeddings
        if duration_ms < 2000:
            logger.warning(f"Audio too short for embedding: {duration_ms}ms")
            return inference_pb2.EmbeddingResponse(
                embedding=[],
                quality_score=0.0,
                duration_ms=duration_ms,
            )

        try:
            result = model(audio_np)

            # Extract embedding vector
            if isinstance(result, dict) and "embedding" in result:
                embedding = result["embedding"].flatten().tolist()
            elif hasattr(result, 'embedding'):
                embedding = np.array(result.embedding).flatten().tolist()
            else:
                embedding = np.array(result).flatten().tolist()

            logger.info(f"Extracted {len(embedding)}-dim embedding from {duration_ms:.0f}ms audio")

            return inference_pb2.EmbeddingResponse(
                embedding=embedding[:192],  # CAM++ outputs 192-dim
                quality_score=min(1.0, duration_ms / 5000),
                duration_ms=duration_ms,
            )

        except Exception as e:
            logger.error(f"Embedding extraction error: {e}")
            return inference_pb2.EmbeddingResponse(
                embedding=[],
                quality_score=0.0,
                duration_ms=duration_ms,
            )

    def ExtractEmbedding(self, request, context):
        """Extract 192-dim speaker embedding from audio using CAM++."""
        return self._extract_embedding_internal(request)

    def VerifySpeaker(self, request, context):
        """Verify if audio matches a reference embedding using cosine similarity."""
        audio_data = request.audio_data
        reference_embedding = np.array(request.reference_embedding)

        # Extract embedding from audio (no context collision)
        extract_result = self._extract_embedding_internal(
            inference_pb2.ExtractEmbeddingRequest(
                audio_data=audio_data,
                sample_rate=16000.0,
            ),
        )

        if not extract_result.embedding:
            return inference_pb2.VerifySpeakerResponse(
                similarity_score=0.0,
                threshold=0.0,
                accepted=False,
            )

        test_embedding = np.array(extract_result.embedding)

        # Cosine similarity
        similarity = float(np.dot(reference_embedding, test_embedding) /
                          (np.linalg.norm(reference_embedding) * np.linalg.norm(test_embedding) + 1e-8))

        # Raw threshold (Z-Norm applied in Rust)
        raw_threshold = 0.5
        accepted = similarity > raw_threshold

        return inference_pb2.VerifySpeakerResponse(
            similarity_score=similarity,
            threshold=raw_threshold,
            accepted=accepted,
        )

    def HealthCheck(self, request, context):
        """Report model loading status."""
        state = self._model_loader.state
        return inference_pb2.HealthCheckResponse(
            ready=state.asr_ready and state.speaker_ready,
            vad_loaded=state.vad_ready,
            asr_loaded=state.asr_ready,
            speaker_loaded=state.speaker_ready,
            error_detail=state.error_detail,
        )
