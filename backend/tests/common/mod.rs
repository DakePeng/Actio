use std::sync::Arc;

use actio_asr::domain::types::AudioSession;
use actio_asr::engine::audio_coordinator::AudioCoordinator;
use actio_asr::engine::circuit_breaker::CircuitBreaker;
use actio_asr::engine::metrics::Metrics;
use actio_asr::engine::transcript_aggregator::TranscriptAggregator;
use actio_asr::repository::{db, session};
use sqlx::PgPool;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

static TEST_POOL: OnceCell<PgPool> = OnceCell::const_new();

pub async fn test_pool() -> PgPool {
    TEST_POOL
        .get_or_init(|| async {
            dotenvy::dotenv().ok();

            let database_url = std::env::var("TEST_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .expect(
                    "Set TEST_DATABASE_URL or DATABASE_URL before running integration tests. \
                     The local default from .env is postgres://actio:actio@localhost:5433/actio",
                );

            let pool = db::create_pool(&database_url)
                .await
                .expect("failed to connect test database");

            db::run_migrations(&pool)
                .await
                .expect("failed to run test migrations");

            pool
        })
        .await
        .clone()
}

pub fn new_tenant_id() -> Uuid {
    Uuid::new_v4()
}

pub async fn create_test_session(pool: &PgPool, tenant_id: Uuid) -> AudioSession {
    session::create_session(pool, tenant_id, "microphone", "realtime")
        .await
        .expect("failed to create test session")
}

pub struct TestAppDeps {
    pub coordinator: Arc<AudioCoordinator>,
    pub aggregator: Arc<TranscriptAggregator>,
    pub circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    pub metrics: Arc<Metrics>,
}

pub fn test_app_deps(pool: &PgPool) -> TestAppDeps {
    TestAppDeps {
        coordinator: Arc::new(AudioCoordinator::new()),
        aggregator: Arc::new(TranscriptAggregator::new(pool.clone())),
        circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new())),
        metrics: Arc::new(Metrics::new()),
    }
}
