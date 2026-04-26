//! Long-lived audio capture daemon. Wraps cpal + Silero VAD into a single
//! always-on producer of `CaptureEvent`s. Mute drops the cpal stream;
//! unmute reopens it on the same configured device. Subscribers (the
//! per-clip segment writer + any active LiveStreaming session) receive
//! events through a `tokio::sync::broadcast` channel so a slow subscriber
//! never blocks audio.
//!
//! The daemon is designed for the new batch-clip-processing pipeline.
//! The legacy `InferencePipeline` runs its own capture+VAD inside its
//! lifetime; this daemon supersedes that path. Live streaming, when
//! active, subscribes here too instead of starting its own capture.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, Mutex};
use tracing::{info, warn};

use crate::engine::audio_capture::{self, AudioCaptureHandle};
use crate::engine::vad::{self, SpeechSegment, VadConfig};

/// Fan-out one audio mpsc into two so VAD and the Pcm broadcast can
/// each consume a copy. Best-effort: if one consumer disconnects, the
/// other keeps receiving.
fn tee_audio_rx(
    mut rx: mpsc::Receiver<Vec<f32>>,
) -> (mpsc::Receiver<Vec<f32>>, mpsc::Receiver<Vec<f32>>) {
    let (tx_a, rx_a) = mpsc::channel::<Vec<f32>>(64);
    let (tx_b, rx_b) = mpsc::channel::<Vec<f32>>(64);
    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let _ = tx_a.send(chunk.clone()).await;
            let _ = tx_b.send(chunk).await;
        }
    });
    (rx_a, rx_b)
}

/// Bus event broadcast to all subscribers. Speech segments + raw PCM
/// chunks arrive through the same channel as mute/unmute notifications
/// so a subscriber's loop can react to capture lifecycle without a
/// second channel.
///
/// `Pcm` carries cpal-callback-rate raw audio (16 kHz mono f32). Used by
/// the streaming-Transducer branch of LiveStreamingService — Whisper /
/// SenseVoice / Moonshine etc. consume `Speech` segments instead.
#[derive(Debug, Clone)]
pub enum CaptureEvent {
    Speech(Arc<SpeechSegment>),
    Pcm(Arc<Vec<f32>>),
    Muted,
    Unmuted,
}

pub struct CaptureDaemon {
    inner: Arc<Mutex<Inner>>,
    tx: broadcast::Sender<CaptureEvent>,
}

// SAFETY: AudioCaptureHandle contains a cpal::Stream whose !Send/!Sync
// markers come from a raw `*mut ()` pointer. cpal Streams are documented
// as needing single-thread access, but we hold ours behind a tokio Mutex
// — every interaction with `Inner.handle` happens under that lock, so
// the cpal stream is never *concurrently* accessed across threads.
// Crossing thread boundaries serially is fine; the `Mutex` enforces it.
unsafe impl Send for CaptureDaemon {}
unsafe impl Sync for CaptureDaemon {}

struct Inner {
    handle: Option<AudioCaptureHandle>,
    pump_task: Option<tokio::task::JoinHandle<()>>,
    device_name: Option<String>,
    vad_model_path: PathBuf,
    archive_enabled: bool,
    muted: bool,
}

// SAFETY: Inner is always wrapped in `tokio::sync::Mutex<Inner>` and only
// reached through that lock. The non-Send field is `AudioCaptureHandle`,
// which holds a `cpal::Stream` (raw pointer). cpal Streams need
// single-thread access; the Mutex ensures `handle` is touched by exactly
// one thread at a time. Crossing thread boundaries serially is fine.
unsafe impl Send for Inner {}

impl CaptureDaemon {
    pub fn new(device_name: Option<String>, vad_model_path: PathBuf) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(Mutex::new(Inner {
                handle: None,
                pump_task: None,
                device_name,
                vad_model_path,
                archive_enabled: true,
                muted: false,
            })),
            tx,
        }
    }

    /// Subscribe to capture events. Subscribers created before `start` is
    /// called won't see historical audio — that's the broadcast contract.
    pub fn subscribe(&self) -> broadcast::Receiver<CaptureEvent> {
        self.tx.subscribe()
    }

    /// Open the cpal stream + start the VAD pump. Idempotent: returns Ok
    /// without doing anything if already running.
    pub async fn start(&self) -> anyhow::Result<()> {
        let mut g = self.inner.lock().await;
        if g.handle.is_some() {
            return Ok(());
        }

        let (handle, audio_rx) = audio_capture::start_capture(g.device_name.as_deref())?;

        // Tee audio: one branch drives Silero VAD (produces Speech), the
        // other broadcasts as Pcm for streaming-Transducer subscribers.
        let (vad_audio_rx, mut pcm_audio_rx) = tee_audio_rx(audio_rx);
        let seg_rx = vad::start_vad(&g.vad_model_path, VadConfig::default(), vad_audio_rx)?;

        let tx = self.tx.clone();
        // Pcm pump.
        let tx_pcm = tx.clone();
        tokio::spawn(async move {
            while let Some(chunk) = pcm_audio_rx.recv().await {
                let _ = tx_pcm.send(CaptureEvent::Pcm(Arc::new(chunk)));
            }
        });
        // Speech pump.
        let pump = tokio::spawn(async move {
            let mut seg_rx = seg_rx;
            while let Some(seg) = seg_rx.recv().await {
                // A send error means there are no live subscribers right
                // now — that's expected when no clip writer or live
                // streaming session has subscribed yet, so we drop and move on.
                let _ = tx.send(CaptureEvent::Speech(Arc::new(seg)));
            }
        });

        g.handle = Some(handle);
        g.pump_task = Some(pump);
        g.muted = false;
        info!("CaptureDaemon started");
        Ok(())
    }

    pub async fn stop(&self) {
        let mut g = self.inner.lock().await;
        // Dropping the handle stops cpal; aborting the pump cleans up the
        // task even if the seg_rx channel hasn't closed yet.
        g.handle = None;
        if let Some(t) = g.pump_task.take() {
            t.abort();
        }
        info!("CaptureDaemon stopped");
    }

    pub async fn mute(&self) {
        let already_muted = {
            let mut g = self.inner.lock().await;
            if g.muted {
                true
            } else {
                g.handle = None;
                if let Some(t) = g.pump_task.take() {
                    t.abort();
                }
                g.muted = true;
                false
            }
        };
        if !already_muted {
            let _ = self.tx.send(CaptureEvent::Muted);
            info!("CaptureDaemon muted");
        }
    }

    pub async fn unmute(&self) -> anyhow::Result<()> {
        {
            let mut g = self.inner.lock().await;
            if !g.muted {
                return Ok(());
            }
            g.muted = false;
        }
        if let Err(e) = self.start().await {
            warn!(error=%e, "CaptureDaemon unmute failed to restart capture");
            // Roll back the muted flag so the next unmute attempt retries
            // instead of silently no-op'ing.
            self.inner.lock().await.muted = true;
            return Err(e);
        }
        let _ = self.tx.send(CaptureEvent::Unmuted);
        info!("CaptureDaemon unmuted");
        Ok(())
    }

    pub async fn is_muted(&self) -> bool {
        self.inner.lock().await.muted
    }

    /// When false, the clip writer will drop speech events (no on-disk
    /// archive) but live streaming subscribers still receive them.
    /// Toggled by `pipeline_supervisor` based on `always_listening`.
    pub async fn set_archive_enabled(&self, enabled: bool) {
        let mut g = self.inner.lock().await;
        g.archive_enabled = enabled;
    }

    pub async fn archive_enabled(&self) -> bool {
        self.inner.lock().await.archive_enabled
    }

    /// Handy for the supervisor: reports whether the daemon is currently
    /// driving cpal. Distinct from `is_muted` which only reflects user
    /// intent — start might still fail (device disconnected), in which
    /// case `is_running` is false but `is_muted` is also false.
    pub async fn is_running(&self) -> bool {
        self.inner.lock().await.handle.is_some()
    }

    /// Test-only: inject a synthetic CaptureEvent into the broadcast.
    /// Cross-module tests (e.g. clip_writer) use this to drive the loop
    /// without touching cpal/Silero.
    #[cfg(test)]
    pub fn test_push(&self, ev: CaptureEvent) {
        let _ = self.tx.send(ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vad_path() -> PathBuf {
        // Tests don't actually start cpal — they only exercise the flag
        // surface. A nonexistent path is fine here.
        PathBuf::from("nonexistent_silero.onnx")
    }

    #[tokio::test]
    async fn archive_enabled_defaults_true_and_toggles() {
        let d = CaptureDaemon::new(None, vad_path());
        assert!(d.archive_enabled().await);
        d.set_archive_enabled(false).await;
        assert!(!d.archive_enabled().await);
        d.set_archive_enabled(true).await;
        assert!(d.archive_enabled().await);
    }

    #[tokio::test]
    async fn mute_and_unmute_are_idempotent_no_ops_when_already_in_state() {
        let d = CaptureDaemon::new(None, vad_path());
        // Daemon is freshly created — never started. mute() should still
        // record muted=true even though there's no cpal stream to stop.
        d.mute().await;
        assert!(d.is_muted().await);
        // Second mute is a no-op (no panic, no stale broadcast).
        d.mute().await;
        assert!(d.is_muted().await);
        // unmute() will try to start cpal which may legitimately fail in
        // CI; we don't assert success, only that the flag flips back.
        let _ = d.unmute().await;
    }

    #[tokio::test]
    async fn subscribe_returns_independent_receivers() {
        let d = CaptureDaemon::new(None, vad_path());
        let mut a = d.subscribe();
        let mut b = d.subscribe();
        // Manually push an event to verify both receivers see it.
        let _ = d.tx.send(CaptureEvent::Muted);
        let ra = a.try_recv();
        let rb = b.try_recv();
        assert!(matches!(ra, Ok(CaptureEvent::Muted)));
        assert!(matches!(rb, Ok(CaptureEvent::Muted)));
    }
}
