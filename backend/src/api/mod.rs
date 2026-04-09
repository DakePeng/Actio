pub mod label;
pub mod reminder;
pub mod session;
pub mod ws;

use axum::routing::{delete, get, patch, post};
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
        list_sessions,
        get_session,
        end_session,
        get_transcripts,
        get_todo_items,
        create_speaker,
        list_speakers,
        update_speaker,
        delete_speaker,
    ),
    components(schemas(
        CreateSessionRequest,
        SessionResponse,
        CreateSpeakerRequest,
        UpdateSpeakerRequest,
        AudioSession,
        Speaker,
        Transcript,
        TodoItem,
        TodoStatus,
        TodoPriority,
        TodoListResponse,
        Reminder,
        Label,
        CreateLabelRequest,
        PatchLabelRequest,
        PatchReminderRequest,
        AppApiError,
    )),
)]
struct ApiDoc;

pub fn router(state: AppState) -> Router {
    Router::new()
        // health
        .route("/health", get(health))
        // sessions
        .route("/sessions", get(session::list_sessions))
        .route("/sessions", post(session::create_session))
        .route("/sessions/{id}", get(session::get_session))
        .route("/sessions/{id}/end", post(session::end_session))
        .route("/sessions/{id}/transcripts", get(session::get_transcripts))
        .route("/sessions/{id}/todos", get(session::get_todo_items))
        // reminders
        .route("/reminders", get(reminder::list_reminders))
        .route("/reminders", post(reminder::create_reminder))
        .route("/reminders/{id}", get(reminder::get_reminder))
        .route("/reminders/{id}", patch(reminder::patch_reminder))
        .route("/reminders/{id}", delete(reminder::delete_reminder))
        // labels
        .route("/labels", get(label::list_labels))
        .route("/labels", post(label::create_label))
        .route("/labels/{id}", patch(label::patch_label))
        .route("/labels/{id}", delete(label::delete_label))
        // speakers
        .route("/speakers", post(session::create_speaker))
        .route("/speakers", get(session::list_speakers))
        .route("/speakers/{id}", patch(session::update_speaker))
        .route("/speakers/{id}", delete(session::delete_speaker))
        // docs
        .route("/api-docs/openapi.json", get(openapi))
        .route("/ws", get(ws::ws_session))
        .with_state(state)
        .merge(SwaggerUi::new("/docs"))
}

async fn health(State(state): State<AppState>) -> Json<HealthSummary> {
    let worker_state = if state.inference_router.is_some() {
        "available"
    } else {
        "degraded"
    }
    .to_string();

    Json(HealthSummary {
        active_sessions: state.metrics.active_sessions.load(Ordering::Relaxed),
        uptime_secs: state.metrics.uptime_secs(),
        worker_state,
        local_route_count: state.metrics.local_route_count.load(Ordering::Relaxed),
        worker_error_count: state.metrics.worker_error_count.load(Ordering::Relaxed),
        unknown_speaker_count: state.metrics.unknown_speaker_count.load(Ordering::Relaxed),
    })
}

async fn openapi() -> Json<utoipa::openapi::OpenApi> {
    Json(ApiDoc::openapi())
}
