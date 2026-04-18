use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Audio device info for API response
#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

/// Lists available audio input devices
pub fn list_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|d| d.name().ok());

    host.input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    let name = d.name().ok()?;
                    Some(AudioDeviceInfo {
                        is_default: default_name.as_deref() == Some(&name),
                        name,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Metrics for audio capture
pub struct AudioCaptureMetrics {
    pub frames_captured: AtomicU64,
    pub frames_dropped: AtomicU64,
}

/// Handle to a running audio capture. Drop to stop.
pub struct AudioCaptureHandle {
    _stream: cpal::Stream,
    stop_flag: Arc<AtomicBool>,
    pub metrics: Arc<AudioCaptureMetrics>,
}

impl Drop for AudioCaptureHandle {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// Simple linear interpolation resampler (e.g. 48kHz → 16kHz)
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let output_len = (input.len() as f64 / ratio).ceil() as usize;
    (0..output_len)
        .map(|i| {
            let src = i as f64 * ratio;
            let idx = src as usize;
            let frac = (src - idx as f64) as f32;
            let a = input[idx.min(input.len() - 1)];
            let b = input[(idx + 1).min(input.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

/// Start capturing audio from the specified (or default) input device.
/// Returns a handle (drop to stop) and a tokio mpsc::Receiver<Vec<f32>> of 16kHz mono f32 audio chunks.
///
/// The device is opened at its native sample rate and channel count.
/// Audio is downmixed to mono and resampled to 16kHz in the callback.
pub fn start_capture(
    device_name: Option<&str>,
) -> anyhow::Result<(AudioCaptureHandle, mpsc::Receiver<Vec<f32>>)> {
    let host = cpal::default_host();

    let device = match device_name {
        Some(name) => host
            .input_devices()?
            .find(|d| d.name().ok().as_deref() == Some(name))
            .ok_or_else(|| anyhow::anyhow!("Audio device not found: {}", name))?,
        None => host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No default audio input device"))?,
    };

    let device_name_str = device.name().unwrap_or_else(|_| "unknown".into());
    info!(device = %device_name_str, "Starting audio capture");

    // Use device's preferred config — resample to 16kHz mono in the callback
    let supported = device.default_input_config()?;
    let device_rate = supported.sample_rate().0;
    let device_channels = supported.channels() as usize;

    info!(
        sample_rate = device_rate,
        channels = device_channels,
        format = ?supported.sample_format(),
        "Device native config"
    );

    let config: cpal::StreamConfig = supported.into();

    // Crossbeam channel: sync, bounded. Sized to hold several minutes of
    // audio so the capture can run ahead of the recognizer during model load
    // (wake-from-hibernation). The recognizer catches up faster than real-time
    // once it's ready, so the backlog drains quickly.
    let (cb_tx, cb_rx): (Sender<Vec<f32>>, Receiver<Vec<f32>>) = bounded(8000);

    let metrics = Arc::new(AudioCaptureMetrics {
        frames_captured: AtomicU64::new(0),
        frames_dropped: AtomicU64::new(0),
    });
    let metrics_cb = metrics.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_cb = stop_flag.clone();

    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _info: &cpal::InputCallbackInfo| {
            if stop_cb.load(Ordering::Relaxed) {
                return;
            }

            // 1. Downmix to mono
            let mono: Vec<f32> = if device_channels > 1 {
                data.chunks(device_channels)
                    .map(|frame| frame.iter().sum::<f32>() / device_channels as f32)
                    .collect()
            } else {
                data.to_vec()
            };

            // 2. Resample to 16kHz
            let resampled = if device_rate != 16000 {
                resample_linear(&mono, device_rate, 16000)
            } else {
                mono
            };

            metrics_cb
                .frames_captured
                .fetch_add(resampled.len() as u64, Ordering::Relaxed);
            if cb_tx.try_send(resampled).is_err() {
                metrics_cb.frames_dropped.fetch_add(1, Ordering::Relaxed);
            }
        },
        move |err| {
            warn!(error = %err, "Audio capture error");
        },
        None,
    )?;

    stream.play()?;
    info!(
        target_rate = 16000,
        "Audio capture started (resampling from {}Hz)", device_rate
    );

    // Bridge: crossbeam sync channel → tokio mpsc async channel
    let (tx, rx) = mpsc::channel::<Vec<f32>>(8000);
    let stop_bridge = stop_flag.clone();
    tokio::spawn(async move {
        let mut chunk_count: u64 = 0;
        let mut rms_sum: f64 = 0.0;
        let mut sample_count: u64 = 0;
        loop {
            if stop_bridge.load(Ordering::Relaxed) {
                break;
            }
            match cb_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(chunk) => {
                    // Periodic audio level diagnostic (every ~3s at 16kHz)
                    for &s in &chunk {
                        rms_sum += (s as f64) * (s as f64);
                    }
                    sample_count += chunk.len() as u64;
                    chunk_count += 1;
                    if chunk_count % 150 == 0 {
                        let rms = if sample_count > 0 {
                            (rms_sum / sample_count as f64).sqrt()
                        } else {
                            0.0
                        };
                        info!(
                            rms = format!("{rms:.6}"),
                            samples = sample_count,
                            "Audio level check"
                        );
                        rms_sum = 0.0;
                        sample_count = 0;
                    }

                    if tx.send(chunk).await.is_err() {
                        break; // consumer dropped
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let handle = AudioCaptureHandle {
        _stream: stream,
        stop_flag,
        metrics,
    };

    Ok((handle, rx))
}
