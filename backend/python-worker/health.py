"""Health check module for the gRPC worker."""
from grpc_health.v1 import health, health_pb2


def build_health_servicer() -> health.HealthServicer:
    """Create and configure a gRPC health servicer reporting SERVING status."""
    servicer = health.HealthServicer()
    servicer.set("", health_pb2.HealthCheckResponse.SERVING)
    return servicer
