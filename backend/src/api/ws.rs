use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Query, State},
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use uuid::Uuid;
use tracing::{info, warn};
use std::sync::atomic::Ordering;

use crate::AppState;
use crate::repository::session;

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
    let session_id = match params.session_id {
        Some(session_id) => match session::get_session(&state.pool, session_id).await {
            Ok(_) => session_id,
            Err(error) => {
                warn!(%session_id, %error, "WebSocket requested unknown session");
                return axum::http::StatusCode::NOT_FOUND.into_response();
            }
        },
        None => match session::create_session(
            &state.pool,
            params.tenant_id.unwrap_or(Uuid::nil()),
            params.source_type.as_deref().unwrap_or("microphone"),
            params.mode.as_deref().unwrap_or("realtime"),
        )
        .await
        {
            Ok(session) => session.id,
            Err(error) => {
                warn!(%error, "Failed to create session for websocket");
                return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        },
    };

    ws.on_upgrade(move |socket| handle_socket(socket, state, session_id))
}

async fn handle_socket(socket: WebSocket, state: AppState, session_id: Uuid) {
    let session_key = session_id.to_string();
    info!(%session_id, "WebSocket session started");

    state.coordinator.create_session(session_key.clone()).await;
    state.metrics.active_sessions.fetch_add(1, Ordering::Relaxed);

    let (mut sender, mut receiver) = socket.split();

    // Receive audio chunks from client
    let coordinator = state.coordinator.clone();
    let inference_router = state.inference_router.clone();
    let aggregator = state.aggregator.clone();
    let metrics = state.metrics.clone();
    let session_for_recv = session_key.clone();
    let recv_task = tokio::spawn(async move {
        let mut seq = 0;
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Binary(data) = msg {
                metrics.total_chunks_received.fetch_add(1, Ordering::Relaxed);
                let ready = coordinator.buffer_chunk(
                    &session_for_recv,
                    seq,
                    seq as i64 * 600,
                    data.to_vec(),
                ).await;

                if let Some(router) = &inference_router {
                    if !ready.is_empty() {
                        let request_chunks = ready
                            .into_iter()
                            .map(|chunk| {
                                (
                                    chunk.data,
                                    chunk.timestamp_ms,
                                    chunk.sequence_num,
                                    session_for_recv.clone(),
                                )
                            })
                            .collect::<Vec<_>>();

                        match router.route_asr(request_chunks).await {
                            Ok(results) => {
                                for result in results {
                                    let store_result = if result.is_final {
                                        aggregator
                                            .add_final(
                                                session_id,
                                                &result.text,
                                                result.start_ms,
                                                result.end_ms,
                                                None,
                                            )
                                            .await
                                    } else {
                                        aggregator
                                            .add_partial(
                                                session_id,
                                                &result.text,
                                                result.start_ms,
                                                result.end_ms,
                                                None,
                                            )
                                            .await
                                    };

                                    if let Err(error) = store_result {
                                        warn!(%session_id, %error, "Failed to persist transcript");
                                    }
                                }
                            }
                            Err(error) => {
                                warn!(%session_id, %error, "ASR request failed");
                            }
                        }
                    }
                } else {
                    warn!(%session_id, "Realtime inference is unavailable");
                }
                seq += 1;
            }
        }
    });

    // Send heartbeats back to client
    let send_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if sender.send(Message::Ping(vec![].into())).await.is_err() {
                break;
            }
        }
    });

    tokio::select! {
        _ = recv_task => info!(%session_id, "WebSocket receiver closed"),
        _ = send_task => warn!(%session_id, "WebSocket sender closed"),
    }

    state.coordinator.remove_session(&session_key).await;
    state.metrics.active_sessions.fetch_sub(1, Ordering::Relaxed);
    info!(%session_id, "WebSocket session ended");
}
