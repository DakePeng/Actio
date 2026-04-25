//! On-demand live streaming service for dictation and translation.
//!
//! Subscribes to the always-on `CaptureDaemon` (so capture isn't started
//! a second time), runs per-segment streaming ASR + the existing speaker
//! continuity machine, and broadcasts `transcript` and `speaker_resolved`
//! frames on /ws. **Writes nothing to the database** — the persisted
//! archive comes from `BatchProcessor`.
//!
//! ## Migration status
//!
//! This module is the architectural seam for Plan Task 11. The actual
//! per-segment ASR + speaker-id loop is currently still owned by
//! `engine::inference_pipeline::InferencePipeline`. The supervisor
//! refactor (Plan Task 12) is what swaps the live path over to subscribe
//! to `CaptureDaemon` via this service. Until then, `LiveStreamingService`
//! holds the start/stop API and the broadcast subscription, with the
//! actual ASR loop documented as a TODO at the call site of `start`.
//!
//! Keeping the shape concrete now lets the supervisor wire to a stable
//! interface; the loop body is mechanical port work tracked separately.

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine::capture_daemon::{CaptureDaemon, CaptureEvent};
use crate::engine::inference_pipeline::SpeakerIdConfig;

/// User-visible mode the live streaming service is currently serving.
/// Not strictly necessary for the streaming loop itself but useful for
/// logging and for the supervisor to decide when to stop the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveMode {
    Dictation,
    Translation,
    /// Both dictation and translation are active concurrently — same
    /// streaming pipeline serves both. Stops only when both turn off.
    Both,
}

pub struct LiveStreamingService {
    inner: Arc<Mutex<Inner>>,
    daemon: Arc<CaptureDaemon>,
}

struct Inner {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    session_id: Option<Uuid>,
    mode: Option<LiveMode>,
}

impl LiveStreamingService {
    pub fn new(daemon: Arc<CaptureDaemon>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                cancel: None,
                session_id: None,
                mode: None,
            })),
            daemon,
        }
    }

    pub async fn is_running(&self) -> bool {
        self.inner.lock().await.cancel.is_some()
    }

    pub async fn current_mode(&self) -> Option<LiveMode> {
        self.inner.lock().await.mode
    }

    pub async fn current_session(&self) -> Option<Uuid> {
        self.inner.lock().await.session_id
    }

    /// Spin up the live streaming loop. The CaptureDaemon must already be
    /// running (the supervisor handles that). Idempotent — calling start a
    /// second time with the same session updates `mode` and returns Ok.
    pub async fn start(
        &self,
        session_id: Uuid,
        mode: LiveMode,
        _speaker_id_cfg: SpeakerIdConfig,
        _live_asr_model: Option<String>,
    ) -> anyhow::Result<()> {
        let mut g = self.inner.lock().await;
        if g.cancel.is_some() {
            // Already running — just update the active mode (e.g.
            // dictation already running, translation just started → Both).
            g.mode = Some(merge_mode(g.mode, mode));
            return Ok(());
        }

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        g.cancel = Some(cancel_tx);
        g.session_id = Some(session_id);
        g.mode = Some(mode);

        let mut events = self.daemon.subscribe();
        tokio::spawn(async move {
            // TODO(plan task 11/12): port the per-segment ASR + continuity
            // pipeline out of `engine::inference_pipeline`. Until that
            // happens the service consumes events but does not produce
            // transcripts — `InferencePipeline` remains the live path
            // wired through `api::session` and `api::translate`. The
            // supervisor refactor in Plan Task 12 is what flips the
            // dictation/translation handlers to call this service
            // instead, at which point the body of this loop becomes the
            // real per-segment ASR + WS broadcast.
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => break,
                    ev = events.recv() => {
                        match ev {
                            Ok(CaptureEvent::Speech(_seg)) => {
                                // Reserved for the per-segment ASR call —
                                // see the TODO above.
                            }
                            Ok(CaptureEvent::Muted) | Ok(CaptureEvent::Unmuted) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                warn!(skipped = n, "LiveStreaming lagged on broadcast channel");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
            info!(%session_id, "LiveStreamingService loop exited");
        });

        Ok(())
    }

    pub async fn stop(&self) {
        let mut g = self.inner.lock().await;
        if let Some(tx) = g.cancel.take() {
            let _ = tx.send(());
        }
        g.session_id = None;
        g.mode = None;
        info!("LiveStreamingService stopped");
    }

    /// Stop only if the requested mode was the only thing keeping the
    /// service running. Useful when dictation ends but translation stays
    /// on (and vice versa).
    pub async fn stop_mode(&self, mode: LiveMode) {
        let mut g = self.inner.lock().await;
        let new_mode = match (g.mode, mode) {
            (Some(LiveMode::Both), LiveMode::Dictation) => Some(LiveMode::Translation),
            (Some(LiveMode::Both), LiveMode::Translation) => Some(LiveMode::Dictation),
            _ => None,
        };
        match new_mode {
            Some(m) => {
                g.mode = Some(m);
                info!(remaining=?m, "LiveStreaming dropping one mode");
            }
            None => {
                if let Some(tx) = g.cancel.take() {
                    let _ = tx.send(());
                }
                g.session_id = None;
                g.mode = None;
                info!("LiveStreamingService stopped (last mode released)");
            }
        }
    }
}

fn merge_mode(current: Option<LiveMode>, incoming: LiveMode) -> LiveMode {
    match (current, incoming) {
        (None, m) => m,
        (Some(LiveMode::Dictation), LiveMode::Translation) => LiveMode::Both,
        (Some(LiveMode::Translation), LiveMode::Dictation) => LiveMode::Both,
        (Some(c), _) => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_mode_promotes_to_both() {
        assert_eq!(merge_mode(None, LiveMode::Dictation), LiveMode::Dictation);
        assert_eq!(
            merge_mode(Some(LiveMode::Dictation), LiveMode::Translation),
            LiveMode::Both
        );
        assert_eq!(
            merge_mode(Some(LiveMode::Translation), LiveMode::Dictation),
            LiveMode::Both
        );
        assert_eq!(
            merge_mode(Some(LiveMode::Both), LiveMode::Dictation),
            LiveMode::Both
        );
    }

    #[tokio::test]
    async fn start_then_stop_clears_state() {
        let daemon = Arc::new(CaptureDaemon::new(
            None,
            std::path::PathBuf::from("nonexistent_silero.onnx"),
        ));
        let svc = LiveStreamingService::new(daemon);
        assert!(!svc.is_running().await);

        let session_id = Uuid::new_v4();
        svc.start(session_id, LiveMode::Dictation, SpeakerIdConfig::default(), None)
            .await
            .unwrap();
        assert!(svc.is_running().await);
        assert_eq!(svc.current_session().await, Some(session_id));
        assert_eq!(svc.current_mode().await, Some(LiveMode::Dictation));

        svc.stop().await;
        assert!(!svc.is_running().await);
        assert_eq!(svc.current_session().await, None);
    }

    #[tokio::test]
    async fn stop_mode_keeps_service_running_when_other_mode_active() {
        let daemon = Arc::new(CaptureDaemon::new(
            None,
            std::path::PathBuf::from("nonexistent_silero.onnx"),
        ));
        let svc = LiveStreamingService::new(daemon);
        let session_id = Uuid::new_v4();

        svc.start(session_id, LiveMode::Dictation, SpeakerIdConfig::default(), None)
            .await
            .unwrap();
        // Translation also turns on → mode promotes to Both.
        svc.start(session_id, LiveMode::Translation, SpeakerIdConfig::default(), None)
            .await
            .unwrap();
        assert_eq!(svc.current_mode().await, Some(LiveMode::Both));

        // Dictation ends but translation continues.
        svc.stop_mode(LiveMode::Dictation).await;
        assert!(svc.is_running().await);
        assert_eq!(svc.current_mode().await, Some(LiveMode::Translation));

        // Translation ends → service stops.
        svc.stop_mode(LiveMode::Translation).await;
        assert!(!svc.is_running().await);
    }
}
