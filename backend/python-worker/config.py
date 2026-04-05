from dataclasses import dataclass
import os


@dataclass
class WorkerConfig:
    host: str = "127.0.0.1"
    port: int = 50051
    sample_rate: int = 16000
    chunk_duration_ms: int = 600

    @classmethod
    def from_env(cls) -> "WorkerConfig":
        return cls(
            host=os.getenv("WORKER_HOST", "127.0.0.1"),
            port=int(os.getenv("WORKER_PORT", "50051")),
            sample_rate=int(os.getenv("WORKER_SAMPLE_RATE", "16000")),
            chunk_duration_ms=int(os.getenv("WORKER_CHUNK_MS", "600")),
        )
