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
    let session_id: uuid::Uuid = created_json["id"]
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

#[tokio::test]
async fn docs_ui_points_to_openapi_schema() {
    let pool = common::test_pool().await;
    let deps = common::test_app_deps(&pool);

    let app = api::router(AppState {
        pool: pool.clone(),
        coordinator: deps.coordinator,
        aggregator: deps.aggregator,
        circuit_breaker: deps.circuit_breaker,
        inference_router: None,
        metrics: deps.metrics,
        llm_client: None,
    });

    let openapi_response = app
        .clone()
        .oneshot(
            Request::get("/api-docs/openapi.json")
                .body(Body::empty())
                .expect("openapi request should build"),
        )
        .await
        .expect("router should serve openapi schema");

    assert_eq!(openapi_response.status(), StatusCode::OK);
    let openapi_body = axum::body::to_bytes(openapi_response.into_body(), usize::MAX)
        .await
        .expect("openapi body should be readable");
    let openapi_json: serde_json::Value =
        serde_json::from_slice(&openapi_body).expect("openapi response should be valid json");
    assert_eq!(openapi_json["info"]["title"], "Actio ASR API");

    let docs_response = app
        .oneshot(
            Request::get("/docs/swagger-initializer.js")
                .body(Body::empty())
                .expect("docs request should build"),
        )
        .await
        .expect("router should serve docs ui");

    assert_eq!(docs_response.status(), StatusCode::OK);
    let docs_body = axum::body::to_bytes(docs_response.into_body(), usize::MAX)
        .await
        .expect("docs body should be readable");
    let docs_js = String::from_utf8(docs_body.to_vec()).expect("docs response should be utf-8");

    assert!(docs_js.contains("\"url\": \"/api-docs/openapi.json\""));
}

#[tokio::test]
async fn label_crud_create_list_patch_delete() {
    let pool = common::test_pool().await;
    let deps = common::test_app_deps(&pool);

    let app = api::router(AppState {
        pool: pool.clone(),
        coordinator: deps.coordinator,
        aggregator: deps.aggregator,
        circuit_breaker: deps.circuit_breaker,
        inference_router: None,
        metrics: deps.metrics,
        llm_client: None,
    });


    // POST /labels — use unique name to avoid collisions across test runs
    let label_name = format!("TestLabel-{}", uuid::Uuid::new_v4());
    let create_resp = app
        .clone()
        .oneshot(
            Request::post("/labels")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "name": label_name,
                        "color": "#123456",
                        "bg_color": "#abcdef"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
    let label: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let label_id: uuid::Uuid = label["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(label["name"], label_name);

    // GET /labels
    let list_resp = app
        .clone()
        .oneshot(Request::get("/labels").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = axum::body::to_bytes(list_resp.into_body(), usize::MAX).await.unwrap();
    let labels: serde_json::Value = serde_json::from_slice(&list_body).unwrap();
    assert!(labels.as_array().unwrap().iter().any(|l| l["id"] == label["id"]));

    // PATCH /labels/{id}
    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/labels/{label_id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"name": "Renamed"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);

    // DELETE /labels/{id}
    let del_resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/labels/{label_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn reminder_create_and_patch_status() {
    let pool = common::test_pool().await;
    let deps = common::test_app_deps(&pool);

    let app = api::router(AppState {
        pool: pool.clone(),
        coordinator: deps.coordinator,
        aggregator: deps.aggregator,
        circuit_breaker: deps.circuit_breaker,
        inference_router: None,
        metrics: deps.metrics,
        llm_client: None,
    });

    // POST /reminders
    let create_resp = app
        .clone()
        .oneshot(
            Request::post("/reminders")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({
                        "title": "Buy milk",
                        "priority": "low"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    let body = axum::body::to_bytes(create_resp.into_body(), usize::MAX).await.unwrap();
    let reminder: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let reminder_id: uuid::Uuid = reminder["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(reminder["status"], "open");
    assert_eq!(reminder["labels"].as_array().unwrap().len(), 0);

    // PATCH /reminders/{id} — mark archived
    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/reminders/{reminder_id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"status": "archived"})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let patch_body = axum::body::to_bytes(patch_resp.into_body(), usize::MAX).await.unwrap();
    let patched: serde_json::Value = serde_json::from_slice(&patch_body).unwrap();
    assert_eq!(patched["status"], "archived");
    assert!(patched["archived_at"].as_str().is_some());

    // DELETE /reminders/{id}
    let del_resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/reminders/{reminder_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::NO_CONTENT);
}
