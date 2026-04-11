use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine::audio_capture::{self, AudioCaptureHandle};
use crate::engine::asr;
use crate::engine::model_manager::ModelPaths;
use crate::engine::transcript_aggregator::TranscriptAggregator;
use crate::engine::vad::{self, VadConfig};

pub struct InferencePipeline {
    /// The capture handle — dropping it stops capture
    capture_handle: Option<AudioCaptureHandle>,
    /// Cancel token for the pipeline tasks
    cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

// SAFETY: InferencePipeline is always accessed behind a Mutex, so it will never
// be sent across threads concurrently. cpal::Stream contains *mut () which is
// !Send by default, but the stream is created and destroyed on the same thread
// via the Mutex guard.
unsafe impl Send for InferencePipeline {}

impl InferencePipeline {
    pub fn new() -> Self {
        Self {
            capture_handle: None,
            cancel_tx: None,
        }
    }

    /// Start the pipeline for a session. Returns error if models aren't ready.
    pub fn start_session(
        &mut self,
        session_id: Uuid,
        model_paths: &ModelPaths,
        aggregator: Arc<TranscriptAggregator>,
        device_name: Option<&str>,
        asr_model: Option<&str>,
    ) -> anyhow::Result<()> {
        // 1. Start audio capture
        let (capture_handle, audio_rx) = audio_capture::start_capture(device_name)?;

        // 2. Select ASR model based on user setting and route audio to the
        //    appropriate recognizer.
        //    - Zipformer transducers (zh/en): direct streaming ASR, no VAD.
        //    - SenseVoice: VAD segments fed into offline ASR.
        //    - Other catalog entries are not yet wired.
        let chosen = asr_model.unwrap_or("auto");
        info!(model = chosen, "Selected ASR model");

        let transcript_rx = match chosen {
            "zh_zipformer_14m" => {
                let files = model_paths.zh.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Chinese Zipformer model not downloaded")
                })?;
                asr::start_streaming_asr(files, audio_rx)?
            }
            "en_zipformer_20m" => {
                let files = model_paths.en.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("English Zipformer model not downloaded")
                })?;
                asr::start_streaming_asr(files, audio_rx)?
            }
            "ko_zipformer" => {
                let files = model_paths.ko.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Korean Zipformer model not downloaded")
                })?;
                asr::start_streaming_asr(files, audio_rx)?
            }
            "paraformer_zh_small" => {
                let files = model_paths.paraformer_zh_small.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Paraformer zh-small model not downloaded")
                })?;
                if !model_paths.silero_vad.exists() {
                    return Err(anyhow::anyhow!(
                        "Silero VAD model not downloaded — required for Paraformer"
                    ));
                }
                let segment_rx =
                    vad::start_vad(&model_paths.silero_vad, VadConfig::default(), audio_rx)?;
                asr::start_paraformer_offline_asr(files, segment_rx)?
            }
            "sense_voice_multi" => {
                let files = model_paths.sense_voice.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("SenseVoice model not downloaded")
                })?;
                if !model_paths.silero_vad.exists() {
                    return Err(anyhow::anyhow!(
                        "Silero VAD model not downloaded — required for SenseVoice"
                    ));
                }
                let segment_rx =
                    vad::start_vad(&model_paths.silero_vad, VadConfig::default(), audio_rx)?;
                asr::start_sense_voice_asr(files, segment_rx)?
            }
            "moonshine_tiny_en" => {
                let files = model_paths.moonshine_en.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Moonshine model not downloaded")
                })?;
                if !model_paths.silero_vad.exists() {
                    return Err(anyhow::anyhow!(
                        "Silero VAD model not downloaded — required for Moonshine"
                    ));
                }
                let segment_rx =
                    vad::start_vad(&model_paths.silero_vad, VadConfig::default(), audio_rx)?;
                asr::start_moonshine_asr(files, segment_rx)?
            }
            "funasr_nano" => {
                let files = model_paths.funasr_nano.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("FunASR Nano model not downloaded")
                })?;
                if !model_paths.silero_vad.exists() {
                    return Err(anyhow::anyhow!(
                        "Silero VAD model not downloaded — required for FunASR Nano"
                    ));
                }
                let segment_rx =
                    vad::start_vad(&model_paths.silero_vad, VadConfig::default(), audio_rx)?;
                asr::start_funasr_nano_asr(files, segment_rx)?
            }
            // Default / unknown: fall back to first available streaming Zipformer.
            _ => {
                let files = model_paths
                    .zh
                    .as_ref()
                    .or(model_paths.en.as_ref())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No runtime-supported ASR model available for '{}'",
                            chosen
                        )
                    })?;
                asr::start_streaming_asr(files, audio_rx)?
            }
        };

        // 4. Spawn task to persist transcripts
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let mut transcript_rx = transcript_rx;
            loop {
                tokio::select! {
                    result = transcript_rx.recv() => {
                        match result {
                            Some(t) => {
                                if t.text.trim().is_empty() {
                                    continue;
                                }
                                let start_ms = (t.start_sample as i64 * 1000) / 16000;
                                let end_ms = (t.end_sample as i64 * 1000) / 16000;

                                if t.is_final {
                                    if let Err(e) = aggregator.add_final(
                                        session_id, &t.text, start_ms, end_ms, None,
                                    ).await {
                                        warn!(%session_id, error = %e, "Failed to persist final transcript");
                                    }
                                } else {
                                    // Partials: broadcast to WS only, no DB persist
                                    aggregator.broadcast_partial(&t.text, start_ms, end_ms);
                                }
                            }
                            None => break, // ASR channel closed
                        }
                    }
                    _ = &mut cancel_rx => {
                        info!(%session_id, "Pipeline cancelled");
                        break;
                    }
                }
            }
            info!(%session_id, "Transcript persistence task ended");
        });

        self.capture_handle = Some(capture_handle);
        self.cancel_tx = Some(cancel_tx);

        info!(%session_id, "Inference pipeline started");
        Ok(())
    }

    /// Stop the pipeline (called on session end)
    pub fn stop(&mut self) {
        if let Some(cancel) = self.cancel_tx.take() {
            let _ = cancel.send(());
        }
        self.capture_handle.take(); // Drop stops the stream
    }

    pub fn is_running(&self) -> bool {
        self.capture_handle.is_some()
    }
}

impl Default for InferencePipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for InferencePipeline {
    fn drop(&mut self) {
        self.stop();
    }
}
