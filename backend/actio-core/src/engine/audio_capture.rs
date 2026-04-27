use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Audio device info for API response
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
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

/// Common pipeline for an f32-converted buffer: downmix → resample → forward.
/// Pulled out of the capture closure so the per-format dispatch stays minimal.
fn process_chunk(
    data: &[f32],
    device_channels: usize,
    device_rate: u32,
    cb_tx: &Sender<Vec<f32>>,
    metrics: &AudioCaptureMetrics,
) {
    let mono: Vec<f32> = if device_channels > 1 {
        data.chunks(device_channels)
            .map(|frame| frame.iter().sum::<f32>() / device_channels as f32)
            .collect()
    } else {
        data.to_vec()
    };

    let resampled = if device_rate != 16000 {
        resample_linear(&mono, device_rate, 16000)
    } else {
        mono
    };

    metrics
        .frames_captured
        .fetch_add(resampled.len() as u64, Ordering::Relaxed);
    if cb_tx.try_send(resampled).is_err() {
        metrics.frames_dropped.fetch_add(1, Ordering::Relaxed);
    }
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

    let sample_format = supported.sample_format();
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
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Dispatch on the device's native sample format. Hardcoding f32 fails on
    // most macOS built-in mics and many Linux ALSA devices, which deliver i16
    // PCM by default — `build_input_stream::<f32>` would return
    // SampleFormatNotSupported and the pipeline would never start.
    let stream = match sample_format {
        SampleFormat::F32 => {
            // F32 path: match the pre-refactor closure signature exactly so
            // the proven Windows / WASAPI behaviour is unchanged.
            let stop_cb = stop_flag.clone();
            let metrics_cb = metrics.clone();
            let cb_tx_f32 = cb_tx.clone();
            device.build_input_stream(
                &config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    if stop_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    process_chunk(data, device_channels, device_rate, &cb_tx_f32, &metrics_cb);
                },
                move |err| {
                    warn!(error = %err, "Audio capture error");
                },
                None,
            )?
        }
        SampleFormat::I16 => {
            let stop_cb = stop_flag.clone();
            let metrics_cb = metrics.clone();
            let cb_tx_i16 = cb_tx.clone();
            device.build_input_stream::<i16, _, _>(
                &config,
                move |data: &[i16], _info: &cpal::InputCallbackInfo| {
                    if stop_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    let f: Vec<f32> = data.iter().map(|s| (*s as f32) / 32768.0).collect();
                    process_chunk(&f, device_channels, device_rate, &cb_tx_i16, &metrics_cb);
                },
                move |err| {
                    warn!(error = %err, "Audio capture error");
                },
                None,
            )?
        }
        SampleFormat::U16 => {
            let stop_cb = stop_flag.clone();
            let metrics_cb = metrics.clone();
            let cb_tx_u16 = cb_tx.clone();
            device.build_input_stream::<u16, _, _>(
                &config,
                move |data: &[u16], _info: &cpal::InputCallbackInfo| {
                    if stop_cb.load(Ordering::Relaxed) {
                        return;
                    }
                    let f: Vec<f32> = data
                        .iter()
                        .map(|s| (*s as f32 - 32768.0) / 32768.0)
                        .collect();
                    process_chunk(&f, device_channels, device_rate, &cb_tx_u16, &metrics_cb);
                },
                move |err| {
                    warn!(error = %err, "Audio capture error");
                },
                None,
            )?
        }
        fmt => {
            return Err(anyhow::anyhow!(
                "Unsupported audio sample format: {fmt:?}. Expected F32, I16, or U16."
            ));
        }
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_metrics() -> Arc<AudioCaptureMetrics> {
        Arc::new(AudioCaptureMetrics {
            frames_captured: AtomicU64::new(0),
            frames_dropped: AtomicU64::new(0),
        })
    }

    // ── resample_linear ─────────────────────────────────────────────

    #[test]
    fn resample_identity_when_rates_match() {
        let input: Vec<f32> = (0..1024).map(|i| i as f32 * 0.001).collect();
        let out = resample_linear(&input, 16_000, 16_000);
        assert_eq!(out, input);
    }

    #[test]
    fn resample_empty_returns_empty() {
        let out = resample_linear(&[], 48_000, 16_000);
        assert!(out.is_empty());
    }

    #[test]
    fn resample_single_sample_does_not_panic() {
        // The clamp `idx.min(input.len() - 1)` is the only thing keeping
        // this from OOB-indexing on the second neighbour. Pin the path.
        let out = resample_linear(&[0.42], 48_000, 16_000);
        assert!(!out.is_empty(), "single-sample input still produced output");
        assert!(out.iter().all(|&v| (v - 0.42).abs() < 1e-6));
    }

    #[test]
    fn resample_48k_to_16k_outputs_third_length() {
        // Production path: 48 kHz → 16 kHz, ratio 3.0. Output length is
        // `ceil(input_len / 3)`. A 30-sample input produces 10 samples.
        let input: Vec<f32> = (0..30).map(|i| i as f32).collect();
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn resample_48k_to_16k_is_monotonic_for_monotonic_input() {
        // A linearly-increasing input must remain monotonically
        // non-decreasing after linear interpolation downsampling — any
        // off-by-one would shuffle samples and break this.
        let input: Vec<f32> = (0..300).map(|i| i as f32).collect();
        let out = resample_linear(&input, 48_000, 16_000);
        for w in out.windows(2) {
            assert!(w[1] >= w[0], "monotonicity violated: {} -> {}", w[0], w[1]);
        }
    }

    #[test]
    fn resample_16k_to_48k_outputs_triple_length() {
        // Upsampling direction. Same code path; verifies the ratio < 1
        // case doesn't truncate.
        let input: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let out = resample_linear(&input, 16_000, 48_000);
        assert_eq!(out.len(), 30);
    }

    #[test]
    fn resample_last_sample_no_oob() {
        // An exact-multiple input length tests the boundary clamp on the
        // last interpolation pair (idx + 1 would otherwise == len).
        let input = vec![1.0_f32; 6];
        let out = resample_linear(&input, 48_000, 16_000);
        // 2 samples after 3:1 downsample, all should be the constant 1.0.
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|&v| (v - 1.0).abs() < 1e-6));
    }

    // ── process_chunk ───────────────────────────────────────────────

    #[test]
    fn process_chunk_mono_passthrough() {
        let (tx, rx) = bounded::<Vec<f32>>(8);
        let metrics = fresh_metrics();
        let input = vec![0.5_f32; 100];
        process_chunk(&input, 1, 16_000, &tx, &metrics);
        let received = rx.try_recv().expect("chunk should be forwarded");
        assert_eq!(received, input);
        assert_eq!(metrics.frames_captured.load(Ordering::Relaxed), 100);
        assert_eq!(metrics.frames_dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn process_chunk_stereo_averages_pairs() {
        // Stereo frame: [L0, R0, L1, R1, …] → mono averages
        // (L+R)/2 per frame.
        let (tx, rx) = bounded::<Vec<f32>>(8);
        let metrics = fresh_metrics();
        let interleaved = vec![1.0, 0.0, 0.5, 0.5, 0.0, 1.0]; // 3 stereo frames
        process_chunk(&interleaved, 2, 16_000, &tx, &metrics);
        let received = rx.try_recv().unwrap();
        assert_eq!(received, vec![0.5, 0.5, 0.5]);
    }

    #[test]
    fn process_chunk_six_channel_averages_to_mono() {
        let (tx, rx) = bounded::<Vec<f32>>(8);
        let metrics = fresh_metrics();
        // Two 6-channel frames; per-frame sum is 6.0 → mono 1.0 each.
        let interleaved = vec![1.0_f32; 12];
        process_chunk(&interleaved, 6, 16_000, &tx, &metrics);
        let received = rx.try_recv().unwrap();
        assert_eq!(received.len(), 2);
        assert!(received.iter().all(|&v| (v - 1.0).abs() < 1e-6));
    }

    #[test]
    fn process_chunk_resample_skipped_at_16k() {
        // At native 16 kHz the resampler must not run — output length
        // equals mono length, with no interpolation noise. Use a non-
        // multiple length to be sure.
        let (tx, rx) = bounded::<Vec<f32>>(8);
        let metrics = fresh_metrics();
        let input = vec![0.25_f32; 97];
        process_chunk(&input, 1, 16_000, &tx, &metrics);
        let received = rx.try_recv().unwrap();
        assert_eq!(received.len(), 97);
        // Counter should reflect the post-resample length (i.e., 97), so
        // it'd be wrong if a no-op resample silently produced 96 or 98.
        assert_eq!(metrics.frames_captured.load(Ordering::Relaxed), 97);
    }

    #[test]
    fn process_chunk_full_channel_increments_dropped_counter() {
        // bounded(0) is rendezvous-only; with no receiver waiting,
        // try_send returns Err and the drop counter increments. Note
        // the production code increments `frames_captured` *before*
        // attempting the send, so it counts processed (not delivered)
        // frames — both counters should advance on the failure path.
        let (tx, _rx) = bounded::<Vec<f32>>(0);
        drop(_rx);
        let metrics = fresh_metrics();
        process_chunk(&[0.5_f32; 50], 1, 16_000, &tx, &metrics);
        assert_eq!(metrics.frames_dropped.load(Ordering::Relaxed), 1);
        // `frames_captured` reflects total frames seen by the callback,
        // even if delivery to the consumer failed. Pin the current
        // semantic so a future "fix" doesn't silently flip it.
        assert_eq!(metrics.frames_captured.load(Ordering::Relaxed), 50);
    }

    #[test]
    fn process_chunk_48k_stereo_downmix_then_resample() {
        // End-to-end: 6 stereo frames at 48 kHz → 6 mono samples at
        // 48 kHz → 2 mono samples at 16 kHz (ceil(6/3) under the
        // resampler's output-length formula).
        let (tx, rx) = bounded::<Vec<f32>>(8);
        let metrics = fresh_metrics();
        let interleaved = vec![0.4_f32; 12]; // 6 stereo frames, all 0.4
        process_chunk(&interleaved, 2, 48_000, &tx, &metrics);
        let received = rx.try_recv().unwrap();
        assert_eq!(received.len(), 2);
        assert!(received.iter().all(|&v| (v - 0.4).abs() < 1e-6));
        assert_eq!(metrics.frames_captured.load(Ordering::Relaxed), 2);
    }
}
