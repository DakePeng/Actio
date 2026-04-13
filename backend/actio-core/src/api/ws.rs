use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tracing::{info, warn};
use std::sync::atomic::Ordering;

use crate::AppState;
use crate::repository::session;

#[derive(Serialize)]
struct WsTranscriptEvent {
    kind: &'static str,
    transcript_id: String,
    text: String,
    start_ms: i64,
    end_ms: i64,
    is_final: bool,
    speaker_id: Option<Uuid>,
}

#[derive(Deserialize, Clone, Default)]
pub struct WsSessionParams {
    pub session_id: Option<Uuid>,
    pub tenant_id: Option<Uuid>,
    pub source_type: Option<String>,
    pub mode: Option<String>,
}

pub async fn ws_session(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<WsSessionParams>,
) -> impl IntoResponse {
    // The aggregator is a single global broadcast channel — every WebSocket
    // subscriber receives every transcript regardless of session. So when no
    // session_id is supplied, we treat the connection as a pure listener:
    // it taps the live transcript stream produced by whatever inference
    // pipeline is currently running (typically the always-on global one
    // started in lib.rs) without creating or validating a session row.
    let session_id = match params.session_id {
        Some(sid) => match session::get_session(&state.pool, sid).await {
            Ok(_) => sid,
            Err(error) => {
                warn!(%sid, %error, "WebSocket requested unknown session");
                return axum::http::StatusCode::NOT_FOUND.into_response();
            }
        },
        None => Uuid::nil(), // listen-only — no session attached
    };

    ws.on_upgrade(move |socket| handle_socket(socket, state, session_id))
}

async fn handle_socket(socket: WebSocket, state: AppState, session_id: Uuid) {
    info!(%session_id, "WebSocket session started");
    state.metrics.active_sessions.fetch_add(1, Ordering::Relaxed);

    let (mut sender, mut receiver) = socket.split();

    // TODO(Phase 3-4): Wire cpal audio capture → VAD → ASR pipeline here.
    // For now, accept messages but don't process audio — the inference pipeline
    // doesn't exist yet. Transcript events will be pushed once ASR is integrated.

    let aggregator_rx = state.aggregator.subscribe();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Binary(_) => {
                    // Audio chunks received but inference pipeline not yet connected
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Send transcript events and heartbeats
    let mut transcript_rx = aggregator_rx;
    let send_task = tokio::spawn(async move {
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(15));
        heartbeat.tick().await;

        loop {
            tokio::select! {
                event = transcript_rx.recv() => {
                    match event {
                        Ok(t) => {
                            let msg = WsTranscriptEvent {
                                kind: "transcript",
                                transcript_id: t.id,
                                text: t.text,
                                start_ms: t.start_ms,
                                end_ms: t.end_ms,
                                is_final: t.is_final,
                                speaker_id: t.speaker_id,
                            };
                            match serde_json::to_string(&msg) {
                                Ok(json) => {
                                    if sender.send(Message::Text(json.into())).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => warn!(error = %e, "Failed to serialize transcript event"),
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!(skipped = n, "WebSocket send lagged behind transcript events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = heartbeat.tick() => {
                    if sender.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = recv_task => info!(%session_id, "WebSocket receiver closed"),
        _ = send_task => warn!(%session_id, "WebSocket sender closed"),
    }

    state.metrics.active_sessions.fetch_sub(1, Ordering::Relaxed);
    info!(%session_id, "WebSocket session ended");
}
