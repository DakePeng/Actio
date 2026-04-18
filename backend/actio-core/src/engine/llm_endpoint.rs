use std::net::SocketAddr;

use axum::routing::{get, post};
use axum::Router;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::api::llm::{openai_chat_completions, openai_list_models};
use crate::AppState;

pub struct LocalLlmEndpoint {
    bound_port: Option<u16>,
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl LocalLlmEndpoint {
    pub fn new() -> Self {
        Self {
            bound_port: None,
            shutdown_tx: None,
        }
    }

    pub fn bound_port(&self) -> Option<u16> {
        self.bound_port
    }

    pub async fn start_or_rebind(
        &mut self,
        port: u16,
        state: AppState,
    ) -> Result<(), std::io::Error> {
        if self.bound_port == Some(port) {
            return Ok(());
        }
        self.stop().await;

        let addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .expect("valid socket addr");
        let listener = tokio::net::TcpListener::bind(addr).await?;
        info!(%addr, "Local LLM endpoint listener bound");

        let (tx, mut rx) = watch::channel(false);
        let app: Router = Router::new()
            .route("/v1/models", get(openai_list_models))
            .route("/v1/chat/completions", post(openai_chat_completions))
            .with_state(state);

        tokio::spawn(async move {
            let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
                let _ = rx.changed().await;
            });
            if let Err(e) = serve.await {
                warn!(error = %e, "Local LLM endpoint listener stopped");
            }
        });

        self.bound_port = Some(port);
        self.shutdown_tx = Some(tx);
        Ok(())
    }

    pub async fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
            self.bound_port = None;
        }
    }
}

impl Default for LocalLlmEndpoint {
    fn default() -> Self {
        Self::new()
    }
}
