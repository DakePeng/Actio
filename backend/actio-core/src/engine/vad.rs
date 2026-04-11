use std::path::Path;
use tokio::sync::mpsc;
use tracing::{info, debug};

/// A detected speech segment with its audio data
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub start_sample: usize,
    pub end_sample: usize,
    pub audio: Vec<f32>,
}

/// Configuration for the VAD
pub struct VadConfig {
    pub threshold: f32,
    pub min_silence_duration: f32,
    pub min_speech_duration: f32,
    pub window_size: i32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_silence_duration: 0.3,
            min_speech_duration: 0.25,
            window_size: 512,
        }
    }
}

/// Create a VAD and run it as a task that consumes audio and emits speech segments.
/// Returns an mpsc::Receiver<SpeechSegment>.
///
/// Call this after start_capture() and feed the audio receiver into this function.
///
/// VoiceActivityDetector holds a raw pointer and is !Send. The entire VAD loop
/// (create + process) runs inside a single `spawn_blocking` call so it never
/// needs to cross thread boundaries. Audio arrives via a crossbeam sync channel
/// bridged from the tokio mpsc receiver, and completed segments are sent back
/// via a crossbeam sync channel bridged to the returned tokio mpsc receiver.
pub fn start_vad(
    model_path: &Path,
    config: VadConfig,
    mut audio_rx: mpsc::Receiver<Vec<f32>>,
) -> anyhow::Result<mpsc::Receiver<SpeechSegment>> {
    let vad_config = sherpa_onnx::VadModelConfig {
        silero_vad: sherpa_onnx::SileroVadModelConfig {
            model: Some(model_path.to_string_lossy().to_string()),
            threshold: config.threshold,
            min_silence_duration: config.min_silence_duration,
            min_speech_duration: config.min_speech_duration,
            window_size: config.window_size,
            ..Default::default()
        },
        sample_rate: 16000,
        num_threads: 1,
        provider: Some("cpu".to_string()),
        debug: false,
        ..Default::default()
    };

    let model_path_owned = model_path.to_path_buf();
    let window = config.window_size as usize;

    // Sync bridge: tokio audio_rx → blocking VAD thread
    let (audio_tx, audio_cb_rx) = crossbeam_channel::bounded::<Vec<f32>>(64);
    // Sync bridge: blocking VAD thread → tokio segment consumer
    let (seg_cb_tx, seg_rx) = crossbeam_channel::bounded::<SpeechSegment>(32);

    // Task 1: drain tokio mpsc into crossbeam channel for the blocking thread
    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            if audio_tx.send(chunk).is_err() {
                break; // VAD thread exited
            }
        }
    });

    // Task 2: bridge crossbeam segments back to tokio mpsc
    let (seg_tokio_tx, seg_tokio_rx) = mpsc::channel::<SpeechSegment>(32);
    tokio::spawn(async move {
        loop {
            match seg_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(seg) => {
                    if seg_tokio_tx.send(seg).await.is_err() {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // Task 3: blocking thread owns VoiceActivityDetector for its entire lifetime
    tokio::task::spawn_blocking(move || {
        let vad = match sherpa_onnx::VoiceActivityDetector::create(&vad_config, 60.0) {
            Some(v) => v,
            None => {
                tracing::error!(
                    model = ?model_path_owned,
                    "Failed to create VAD — check model path"
                );
                return;
            }
        };

        info!("Silero VAD initialized");

        let mut total_samples: usize = 0;
        let mut buffer: Vec<f32> = Vec::new();

        loop {
            match audio_cb_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(chunk) => {
                    buffer.extend_from_slice(&chunk);

                    while buffer.len() >= window {
                        let window_data: Vec<f32> = buffer.drain(..window).collect();
                        vad.accept_waveform(&window_data);
                        total_samples += window;

                        while let Some(seg) = vad.front() {
                            let start = seg.start() as usize;
                            let samples = seg.samples().to_vec();
                            let end = start + samples.len();
                            let segment = SpeechSegment {
                                start_sample: start,
                                end_sample: end,
                                audio: samples,
                            };
                            debug!(
                                start = segment.start_sample,
                                end = segment.end_sample,
                                duration_ms = (segment.audio.len() as f32 / 16.0) as u64,
                                "Speech segment detected"
                            );
                            vad.pop();
                            if seg_cb_tx.send(segment).is_err() {
                                info!(total_samples, "VAD thread ended — segment consumer dropped");
                                return;
                            }
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        info!(total_samples, "VAD thread ended");
    });

    Ok(seg_tokio_rx)
}
