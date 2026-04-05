use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Worker unavailable: {0}")]
    WorkerUnavailable(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("gRPC error: {0}")]
    Grpc(#[from] tonic::Status),
}
