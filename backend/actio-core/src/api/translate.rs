use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::session::AppApiError;
use crate::engine::llm_router::LlmRouterError;
use crate::engine::llm_translate::TranslateLineRequest;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct TranslateRequest {
    pub target_lang: String,
    pub lines: Vec<TranslateLineRequestWire>,
}

#[derive(Debug, Deserialize)]
pub struct TranslateLineRequestWire {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct TranslateResponse {
    pub translations: Vec<TranslateLineWire>,
}

#[derive(Debug, Serialize)]
pub struct TranslateLineWire {
    pub id: String,
    pub text: String,
}

/// POST /llm/translate — batch-translate transcript lines.
///
/// Returns 503 with `{"error":"llm_disabled"}` when the router is in
/// `Disabled` mode so the frontend can surface a precise toast.
pub async fn translate_lines(
    State(state): State<AppState>,
    Json(req): Json<TranslateRequest>,
) -> Result<Json<TranslateResponse>, Response> {
    if req.lines.is_empty() {
        return Ok(Json(TranslateResponse {
            translations: vec![],
        }));
    }

    let lines: Vec<TranslateLineRequest> = req
        .lines
        .into_iter()
        .map(|l| TranslateLineRequest {
            id: l.id,
            text: l.text,
        })
        .collect();

    // Serialize against window-extractor LLM calls. Fair FIFO mutex:
    // a translation queued behind a long extraction call simply waits
    // (and vice versa).
    let _guard = state.llm_inflight.lock().await;

    let router = state.router.read().await;
    let result = router.translate_lines(&req.target_lang, lines).await;
    drop(router);
    drop(_guard);

    match result {
        Ok(translations) => Ok(Json(TranslateResponse {
            translations: translations
                .into_iter()
                .map(|t| TranslateLineWire {
                    id: t.id,
                    text: t.text,
                })
                .collect(),
        })),
        Err(LlmRouterError::Disabled) => Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "llm_disabled"})),
        )
            .into_response()),
        Err(other) => Err(AppApiError::Internal(other.to_string()).into_response()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::live_enrollment;
    use crate::engine::llm_downloader::LlmDownloader;
    use crate::engine::llm_endpoint::LocalLlmEndpoint;
    use crate::engine::llm_router::LlmRouter;
    use crate::engine::local_llm_engine::EngineSlot;
    use crate::engine::metrics::Metrics;
    use crate::engine::model_manager::ModelManager;
    use crate::engine::transcript_aggregator::TranscriptAggregator;
    use crate::engine::{app_settings::SettingsManager, inference_pipeline::InferencePipeline};
    use crate::repository::db::run_migrations;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as AxumStatus};
    use axum::routing::post;
    use axum::Router;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;

    /// Build a minimal `AppState` for endpoint integration tests, parameterised
    /// on the LlmRouter variant under test. Inlined here (rather than in a
    /// shared `test_support` module) because no other API tests need it yet
    /// and pulling it out would balloon the test surface beyond this task.
    async fn make_state(router: LlmRouter) -> (AppState, TempDir) {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();

        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().to_path_buf();
        let model_dir = data_dir.join("models");
        std::fs::create_dir_all(&model_dir).unwrap();
        let clips_dir = data_dir.join("audio_clips");
        std::fs::create_dir_all(&clips_dir).unwrap();

        let state = AppState {
            pool,
            aggregator: Arc::new(TranscriptAggregator::new(
                SqlitePoolOptions::new()
                    .connect("sqlite::memory:")
                    .await
                    .unwrap(),
            )),
            metrics: Arc::new(Metrics::new()),
            model_manager: Arc::new(ModelManager::new(model_dir.clone())),
            inference_pipeline: Arc::new(tokio::sync::Mutex::new(InferencePipeline::new())),
            settings_manager: Arc::new(SettingsManager::new(&data_dir)),
            clips_dir,
            live_enrollment: live_enrollment::new_state(),
            enrollment_owned_session: Arc::new(tokio::sync::Mutex::new(None)),
            pipeline_restart: Arc::new(tokio::sync::Notify::new()),
            engine_slot: Arc::new(EngineSlot::new(model_dir.clone())),
            llm_downloader: Arc::new(LlmDownloader::new(model_dir)),
            remote_client_envseed: None,
            router: Arc::new(tokio::sync::RwLock::new(router)),
            llm_inflight: Arc::new(tokio::sync::Mutex::new(())),
            audio_levels: Arc::new(tokio::sync::broadcast::channel::<f32>(8).0),
            llm_endpoint: Arc::new(tokio::sync::Mutex::new(LocalLlmEndpoint::new())),
        };
        (state, tmp)
    }

    fn app(state: AppState) -> Router {
        Router::new()
            .route("/llm/translate", post(translate_lines))
            .with_state(state)
    }

    async fn read_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn returns_503_when_router_disabled() {
        let (state, _tmp) = make_state(LlmRouter::Disabled).await;
        let body = serde_json::json!({
            "target_lang": "zh-CN",
            "lines": [{"id": "line-1", "text": "hello"}],
        });
        let req = Request::builder()
            .method("POST")
            .uri("/llm/translate")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), AxumStatus::SERVICE_UNAVAILABLE);
        let json = read_body(resp).await;
        assert_eq!(json["error"], "llm_disabled");
    }

    #[tokio::test]
    async fn returns_translations_for_stub_router() {
        let (state, _tmp) =
            make_state(LlmRouter::stub_with_translation_suffix(" [zh]")).await;
        let body = serde_json::json!({
            "target_lang": "zh-CN",
            "lines": [
                {"id": "line-1", "text": "first"},
                {"id": "line-2", "text": "second"},
            ],
        });
        let req = Request::builder()
            .method("POST")
            .uri("/llm/translate")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), AxumStatus::OK);
        let json = read_body(resp).await;
        let arr = json["translations"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "line-1");
        assert_eq!(arr[0]["text"], "first [zh]");
        assert_eq!(arr[1]["id"], "line-2");
        assert_eq!(arr[1]["text"], "second [zh]");
    }

    #[tokio::test]
    async fn accepts_non_uuid_ids() {
        // Defensive — keeps this endpoint usable even if a caller sends
        // a synthetic id like `local-…` from `appendLiveTranscript`.
        let (state, _tmp) =
            make_state(LlmRouter::stub_with_translation_suffix(" [zh]")).await;
        let body = serde_json::json!({
            "target_lang": "zh-CN",
            "lines": [{"id": "local-not-a-uuid", "text": "hello"}],
        });
        let req = Request::builder()
            .method("POST")
            .uri("/llm/translate")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app(state).oneshot(req).await.unwrap();
        assert_eq!(resp.status(), AxumStatus::OK);
        let json = read_body(resp).await;
        assert_eq!(json["translations"][0]["id"], "local-not-a-uuid");
    }
}
