import asyncio
import logging
from concurrent import futures

import grpc
from grpc_health.v1 import health_pb2_grpc

from config import WorkerConfig
from health import build_health_servicer
from models.loader import ModelLoader
from services.vad import VADService
from services.asr import ASRService
from services.speaker import SpeakerService
import inference_pb2_grpc

logger = logging.getLogger("actio-worker")


async def serve() -> None:
    config = WorkerConfig.from_env()
    logger.info(f"Starting worker on {config.host}:{config.port}")

    # Load models
    model_loader = ModelLoader()

    # Pre-load models (or defer to first request)
    try:
        model_loader.load_asr_model()
        model_loader.load_speaker_model()
    except Exception as e:
        logger.warning(f"Model pre-loading failed (will retry on first request): {e}")

    server = grpc.aio.server(futures.ThreadPoolExecutor(max_workers=10))

    # Health check service
    health_servicer = build_health_servicer()
    health_pb2_grpc.add_HealthServicer_to_server(health_servicer, server)

    # Inference services
    inference_pb2_grpc.add_VADServiceServicer_to_server(VADService(), server)
    inference_pb2_grpc.add_ASRServiceServicer_to_server(ASRService(model_loader), server)
    inference_pb2_grpc.add_SpeakerServiceServicer_to_server(SpeakerService(model_loader), server)

    server.add_insecure_port(f"{config.host}:{config.port}")
    await server.start()
    logger.info(f"Worker started on {config.host}:{config.port}")

    try:
        await server.wait_for_termination()
    except KeyboardInterrupt:
        logger.info("Shutting down worker...")
        await server.stop(grace=5.0)


if __name__ == "__main__":
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )
    asyncio.run(serve())
