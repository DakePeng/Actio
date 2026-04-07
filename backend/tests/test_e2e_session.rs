mod common;

pub use actio_asr::domain;
pub use actio_asr::engine;
pub use actio_asr::repository;

use std::sync::Arc;

use actio_asr::domain::types::AudioSession;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use tokio::sync::Mutex;
use tower::util::ServiceExt;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub coordinator: Arc<engine::audio_coordinator::AudioCoordinator>,
    pub aggregator: Arc<engine::transcript_aggregator::TranscriptAggregator>,
    pub circuit_breaker: Arc<Mutex<engine::circuit_breaker::CircuitBreaker>>,
    pub inference_router: Option<Arc<engine::inference_router::InferenceRouter>>,
    pub metrics: Arc<engine::metrics::Metrics>,
    pub llm_client: Option<Arc<engine::llm_client::LlmClient>>,
}

#[path = "../src/api/mod.rs"]
mod api;

#[tokio::test]
async fn session_lifecycle_routes_create_end_and_return_ended_session_state() {
    let pool = common::test_pool().await;
    let deps = common::test_app_deps(&pool);
    let tenant_id = common::new_tenant_id();

    let app = api::router(AppState {
        pool: pool.clone(),
        coordinator: deps.coordinator,
        aggregator: deps.aggregator,
        circuit_breaker: deps.circuit_breaker,
        inference_router: None,
        metrics: deps.metrics,
        llm_client: None,
    });

    let create_response = app
        .clone()
        .oneshot(
            Request::post("/sessions")
                .header("content-type", "application/json")
                .header("x-tenant-id", tenant_id.to_string())
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "source_type": "microphone",
                        "mode": "realtime"
                    }))
                    .expect("request body should serialize"),
                ))
                .expect("request should build"),
        )
        .await
        .expect("router should handle create session");

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let create_body = axum::body::to_bytes(create_response.into_body(), usize::MAX)
        .await
        .expect("create body should be readable");
    let created_json: serde_json::Value =
        serde_json::from_slice(&create_body).expect("create response should be valid json");
    let session_id = created_json["id"]
        .as_str()
        .expect("create response should include session id")
        .parse()
        .expect("session id should parse");
    assert!(created_json["started_at"].as_str().is_some());

    let end_response = app
        .clone()
        .oneshot(
            Request::post(format!("/sessions/{session_id}/end"))
                .body(Body::empty())
                .expect("end request should build"),
        )
        .await
        .expect("router should handle end session");

    assert_eq!(end_response.status(), StatusCode::NO_CONTENT);

    let get_response = app
        .oneshot(
            Request::get(format!("/sessions/{session_id}"))
                .body(Body::empty())
                .expect("get request should build"),
        )
        .await
        .expect("router should handle get session");

    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = axum::body::to_bytes(get_response.into_body(), usize::MAX)
        .await
        .expect("get body should be readable");
    let session: AudioSession =
        serde_json::from_slice(&get_body).expect("session response should deserialize");

    assert_eq!(session.id, session_id);
    assert_eq!(session.tenant_id, tenant_id);
    assert!(session.ended_at.is_some());
}
