pub mod session;
pub mod ws;

use axum::routing::{get, post};
use axum::Router;
use axum::Json;
use axum::extract::State;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::AppState;
use crate::api::session::*;
use crate::domain::types::*;
use crate::engine::metrics::HealthSummary;
use std::sync::atomic::Ordering;

#[derive(OpenApi)]
#[openapi(
    info(title = "Actio ASR API", version = "0.1.0"),
    paths(
        create_session,
        get_session,
        end_session,
        get_transcripts,
        get_todo_items,
        create_speaker,
        list_speakers,
    ),
    components(schemas(
        CreateSessionRequest,
        SessionResponse,
        CreateSpeakerRequest,
        AudioSession,
        Speaker,
        Transcript,
        TodoItem,
        TodoStatus,
        TodoPriority,
        AppApiError,
    )),
)]
struct ApiDoc;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", post(session::create_session))
        .route("/sessions/{id}", get(session::get_session))
        .route("/sessions/{id}/end", post(session::end_session))
        .route("/sessions/{id}/transcripts", get(session::get_transcripts))
        .route("/sessions/{id}/todos", get(session::get_todo_items))
        .route("/speakers", post(session::create_speaker))
        .route("/speakers", get(session::list_speakers))
        .route("/api-docs/openapi.json", get(openapi))
        .route("/ws", get(ws::ws_session))
        .with_state(state)
        .merge(SwaggerUi::new("/docs"))
}

async fn health(State(state): State<AppState>) -> Json<HealthSummary> {
    Json(HealthSummary {
        active_sessions: state.metrics.active_sessions.load(Ordering::Relaxed),
        uptime_secs: state.metrics.uptime_secs(),
    })
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}
