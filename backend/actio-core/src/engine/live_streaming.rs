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

use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine::capture_daemon::{CaptureDaemon, CaptureEvent};
use crate::engine::inference_pipeline::SpeakerIdConfig;
use crate::engine::model_manager::ModelPaths;
use crate::engine::transcript_aggregator::TranscriptAggregator;
use crate::engine::vad::SpeechSegment;

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
    aggregator: Arc<TranscriptAggregator>,
}

struct Inner {
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
    session_id: Option<Uuid>,
    mode: Option<LiveMode>,
}

impl LiveStreamingService {
    pub fn new(daemon: Arc<CaptureDaemon>, aggregator: Arc<TranscriptAggregator>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                cancel: None,
                session_id: None,
                mode: None,
            })),
            daemon,
            aggregator,
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
    ///
    /// `live_asr_model` selects which sherpa-onnx offline ASR runs against
    /// each VAD speech segment. SenseVoice is the only catalog id wired in
    /// this commit; other offline models (Whisper / Moonshine / Paraformer
    /// / Zipformer-CTC / FunASR) follow the same pattern from
    /// engine::asr — adding them is a switch-statement update with no
    /// architectural changes. Streaming Zipformer is deliberately not
    /// supported here because it consumes raw audio chunks rather than
    /// VAD segments; live mode falls back to the default offline model
    /// when a streaming-only id is requested.
    pub async fn start(
        &self,
        session_id: Uuid,
        mode: LiveMode,
        _speaker_id_cfg: SpeakerIdConfig,
        live_asr_model: Option<String>,
        model_paths: ModelPaths,
    ) -> anyhow::Result<()> {
        let mut g = self.inner.lock().await;
        if g.cancel.is_some() {
            // Already running — just update the active mode (e.g.
            // dictation already running, translation just started → Both).
            g.mode = Some(merge_mode(g.mode, mode));
            return Ok(());
        }

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        g.cancel = Some(cancel_tx);
        g.session_id = Some(session_id);
        g.mode = Some(mode);
        drop(g);

        let events = self.daemon.subscribe();
        let aggregator = self.aggregator.clone();
        let chosen = live_asr_model.unwrap_or_else(|| "sense_voice_multi".to_string());

        tokio::spawn(async move {
            run_streaming_loop(session_id, &chosen, &model_paths, events, aggregator, cancel_rx)
                .await;
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

/// Bridge CaptureDaemon's broadcast → mpsc Receiver<SpeechSegment>, run
/// per-segment offline ASR, broadcast results on the WS via the
/// aggregator. No DB writes — the persisted archive comes from
/// BatchProcessor's clip-level pass.
async fn run_streaming_loop(
    session_id: Uuid,
    asr_model_id: &str,
    model_paths: &ModelPaths,
    mut events: tokio::sync::broadcast::Receiver<CaptureEvent>,
    aggregator: Arc<TranscriptAggregator>,
    mut cancel_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // 1. Stand up an mpsc bridge: the existing engine::asr offline
    //    helpers consume `mpsc::Receiver<SpeechSegment>`. We translate
    //    capture-daemon broadcast events into that channel.
    let (seg_tx, seg_rx) = mpsc::channel::<SpeechSegment>(32);

    // 2. Start the chosen offline ASR. Each variant returns an mpsc
    //    receiver of TranscriptResult that we drain below.
    let mut transcript_rx = match start_offline_asr(asr_model_id, model_paths, seg_rx) {
        Ok(rx) => rx,
        Err(e) => {
            warn!(%session_id, model = asr_model_id, error = %e,
                "LiveStreaming could not start offline ASR — live transcripts disabled");
            // We still loop on capture events so cancel_rx is reachable
            // and Mute/Unmute notifications don't piles up unbounded.
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => break,
                    ev = events.recv() => {
                        if matches!(ev, Err(tokio::sync::broadcast::error::RecvError::Closed)) {
                            break;
                        }
                    }
                }
            }
            info!(%session_id, "LiveStreamingService idle loop exited");
            return;
        }
    };

    // 3. Forward speech segments from broadcast → mpsc until cancel.
    let forward = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(CaptureEvent::Speech(seg)) => {
                    // Arc<SpeechSegment> from the daemon — we need an
                    // owned SpeechSegment for the asr mpsc. Clone the
                    // inner contents.
                    let owned = SpeechSegment {
                        segment_id: seg.segment_id,
                        start_sample: seg.start_sample,
                        end_sample: seg.end_sample,
                        audio: seg.audio.clone(),
                    };
                    if seg_tx.send(owned).await.is_err() {
                        break; // ASR consumer dropped
                    }
                }
                Ok(CaptureEvent::Muted) | Ok(CaptureEvent::Unmuted) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!(skipped = n, "LiveStreaming lagged on capture broadcast");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // 4. Drain transcripts from the ASR and broadcast on /ws.
    loop {
        tokio::select! {
            _ = &mut cancel_rx => break,
            t = transcript_rx.recv() => {
                match t {
                    Some(t) => {
                        if t.text.trim().is_empty() {
                            continue;
                        }
                        let start_ms = (t.start_sample as i64 * 1000) / 16000;
                        let end_ms = (t.end_sample as i64 * 1000) / 16000;
                        if t.is_final {
                            aggregator.broadcast_final_unpersisted(&t.text, start_ms, end_ms);
                        } else {
                            aggregator.broadcast_partial(&t.text, start_ms, end_ms);
                        }
                    }
                    None => break,
                }
            }
        }
    }

    forward.abort();
    info!(%session_id, "LiveStreamingService loop exited");
}

/// Dispatch one of the offline ASR backends in `engine::asr`. Streaming
/// Zipformer is intentionally excluded — it consumes raw audio chunks,
/// not VAD segments, so it doesn't fit the per-segment shape this loop
/// expects. Unknown ids fall back to SenseVoice (the multi-language
/// default in the catalog).
fn start_offline_asr(
    asr_model_id: &str,
    model_paths: &ModelPaths,
    seg_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<crate::engine::asr::TranscriptResult>> {
    match asr_model_id {
        "whisper_base" => {
            let files = model_paths
                .whisper_base
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Whisper base model not downloaded"))?;
            crate::engine::asr::start_whisper_asr(files, seg_rx)
        }
        "whisper_turbo" => {
            let files = model_paths
                .whisper_turbo
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Whisper turbo model not downloaded"))?;
            crate::engine::asr::start_whisper_asr(files, seg_rx)
        }
        "moonshine_tiny_en" => {
            let files = model_paths
                .moonshine_en
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Moonshine not downloaded"))?;
            crate::engine::asr::start_moonshine_asr(files, seg_rx)
        }
        "paraformer_zh_small" => {
            let files = model_paths
                .paraformer_zh_small
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Paraformer zh-small not downloaded"))?;
            crate::engine::asr::start_paraformer_offline_asr(files, seg_rx)
        }
        "zipformer_ctc_zh_small" => {
            let files = model_paths
                .zipformer_ctc_zh_small
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Zipformer CTC zh-small not downloaded"))?;
            crate::engine::asr::start_zipformer_ctc_asr(files, seg_rx)
        }
        "funasr_nano" => {
            let files = model_paths
                .funasr_nano
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("FunASR Nano not downloaded"))?;
            crate::engine::asr::start_funasr_nano_asr(files, seg_rx)
        }
        // Default: SenseVoice (zh + en + ja + ko + yue, broadly compatible).
        _ => {
            let files = model_paths
                .sense_voice
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("SenseVoice not downloaded"))?;
            crate::engine::asr::start_sense_voice_asr(files, seg_rx)
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

    async fn make_test_aggregator() -> Arc<TranscriptAggregator> {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        Arc::new(TranscriptAggregator::new(pool))
    }

    fn empty_model_paths() -> ModelPaths {
        // Model files don't actually need to exist for the lifecycle tests
        // — start_offline_asr will return Err on missing files and the
        // streaming loop will degrade to "consume events, no transcripts".
        ModelPaths {
            silero_vad: std::path::PathBuf::from("nonexistent_silero.onnx"),
            transducers: std::collections::HashMap::new(),
            speaker_embedding: None,
            sense_voice: None,
            moonshine_en: None,
            paraformer_zh_small: None,
            zipformer_ctc_zh_small: None,
            funasr_nano: None,
            whisper_base: None,
            whisper_turbo: None,
            pyannote_segmentation: None,
        }
    }

    #[tokio::test]
    async fn start_then_stop_clears_state() {
        let daemon = Arc::new(CaptureDaemon::new(
            None,
            std::path::PathBuf::from("nonexistent_silero.onnx"),
        ));
        let agg = make_test_aggregator().await;
        let svc = LiveStreamingService::new(daemon, agg);
        assert!(!svc.is_running().await);

        let session_id = Uuid::new_v4();
        svc.start(
            session_id,
            LiveMode::Dictation,
            SpeakerIdConfig::default(),
            None,
            empty_model_paths(),
        )
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
        let agg = make_test_aggregator().await;
        let svc = LiveStreamingService::new(daemon, agg);
        let session_id = Uuid::new_v4();

        svc.start(
            session_id,
            LiveMode::Dictation,
            SpeakerIdConfig::default(),
            None,
            empty_model_paths(),
        )
        .await
        .unwrap();
        // Translation also turns on → mode promotes to Both.
        svc.start(
            session_id,
            LiveMode::Translation,
            SpeakerIdConfig::default(),
            None,
            empty_model_paths(),
        )
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
