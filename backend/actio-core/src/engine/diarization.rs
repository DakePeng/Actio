use std::path::Path;
use tracing::{debug, info, warn};

/// Result of diarizing a chunk of audio
#[derive(Debug, Clone)]
pub struct DiarizedSegment {
    pub start_sec: f32,
    pub end_sec: f32,
    pub speaker_label: i32, // sherpa-onnx cluster ID
}

/// A speaker embedding extracted from audio
#[derive(Debug, Clone)]
pub struct SpeakerEmbedding {
    pub values: Vec<f32>, // 512-dimensional
    pub duration_samples: usize,
}

/// Diarize a complete audio buffer. Returns segments with speaker labels.
/// Requires full tier models (pyannote + 3D-Speaker).
///
/// `min_audio_seconds`: minimum audio length (recommend 5.0 — shorter audio may crash the C++ library)
pub async fn diarize_audio(
    segmentation_model: &Path,
    embedding_model: &Path,
    audio: &[f32],             // 16kHz mono f32
    num_speakers: Option<i32>, // None = auto-detect
) -> anyhow::Result<Vec<DiarizedSegment>> {
    // Guard: too short for diarization
    if audio.len() < 5 * 16000 {
        warn!(
            audio_samples = audio.len(),
            "Audio too short for diarization (< 5s) — returning empty segments"
        );
        return Ok(vec![]);
    }

    let segmentation_model = segmentation_model.to_path_buf();
    let embedding_model = embedding_model.to_path_buf();
    let audio = audio.to_vec();

    tokio::task::spawn_blocking(move || {
        let config = sherpa_onnx::OfflineSpeakerDiarizationConfig {
            segmentation: sherpa_onnx::OfflineSpeakerSegmentationModelConfig {
                pyannote: sherpa_onnx::OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(segmentation_model.to_string_lossy().to_string()),
                    ..Default::default()
                },
                num_threads: 1,
                provider: Some("cpu".to_string()),
                ..Default::default()
            },
            embedding: sherpa_onnx::SpeakerEmbeddingExtractorConfig {
                model: Some(embedding_model.to_string_lossy().to_string()),
                num_threads: 1,
                provider: Some("cpu".to_string()),
                ..Default::default()
            },
            clustering: sherpa_onnx::FastClusteringConfig {
                num_clusters: num_speakers.unwrap_or(-1),
                threshold: 0.5,
                ..Default::default()
            },
            min_duration_on: 0.3,
            min_duration_off: 0.5,
            ..Default::default()
        };

        let sd = match sherpa_onnx::OfflineSpeakerDiarization::create(&config) {
            Some(sd) => sd,
            None => {
                return Err(anyhow::anyhow!(
                    "Failed to create OfflineSpeakerDiarization — check model paths"
                ));
            }
        };

        info!(
            audio_samples = audio.len(),
            duration_secs = audio.len() as f32 / 16000.0,
            "Running offline speaker diarization"
        );

        let result = match sd.process(&audio) {
            Some(r) => r,
            None => {
                return Err(anyhow::anyhow!(
                    "OfflineSpeakerDiarization::process returned None"
                ));
            }
        };

        let segments = result.sort_by_start_time();

        let diarized: Vec<DiarizedSegment> = segments
            .iter()
            .map(|seg| {
                debug!(
                    start = seg.start,
                    end = seg.end,
                    speaker = seg.speaker,
                    "Diarized segment"
                );
                DiarizedSegment {
                    start_sec: seg.start,
                    end_sec: seg.end,
                    speaker_label: seg.speaker,
                }
            })
            .collect();

        info!(segment_count = diarized.len(), "Diarization complete");
        Ok(diarized)
    })
    .await?
}

type EmbeddingJob = (
    Vec<f32>,
    tokio::sync::oneshot::Sender<anyhow::Result<Vec<f32>>>,
);

/// Per-model-path embedding worker registry. Loading a sherpa-onnx speaker
/// embedding ONNX model is O(hundreds of ms) and was previously done on
/// every VAD segment. Under enrollment's rapid segment flow that piled up
/// inside `run_segment_hook`'s inline `.await`, back-pressuring the VAD
/// bridge until the pipeline visibly stalled. We instead spawn one blocking
/// worker thread per distinct model path, load the extractor once, and
/// reuse it for every subsequent request.
///
/// The registry is LRU-capped at `EMBEDDING_WORKER_CAP` so a user who cycles
/// through speaker-embedding models in the Common Models picker doesn't hold
/// every previous extractor (hundreds of MB each) resident for the life of
/// the process. Eviction closes the worker's sender, which lets the worker
/// thread's `blocking_recv` return `None` and exit cleanly.
const EMBEDDING_WORKER_CAP: usize = 2;
// 32 was the original capacity and matched the steady-state segment rate,
// but it bubbled back-pressure directly into the VAD consumer under bursty
// enrollment load. 128 is comfortably larger than any realistic burst while
// still bounding memory.
const EMBEDDING_WORKER_QUEUE: usize = 128;

struct EmbeddingRegistry {
    // (path, sender) — most-recently-used at the tail.
    entries: Vec<(std::path::PathBuf, tokio::sync::mpsc::Sender<EmbeddingJob>)>,
}

static EMBEDDING_WORKERS: std::sync::OnceLock<std::sync::Mutex<EmbeddingRegistry>> =
    std::sync::OnceLock::new();

fn lock_registry() -> std::sync::MutexGuard<'static, EmbeddingRegistry> {
    let registry = EMBEDDING_WORKERS.get_or_init(|| {
        std::sync::Mutex::new(EmbeddingRegistry {
            entries: Vec::new(),
        })
    });
    // Graceful poison recovery: a trivial critical section here should never
    // panic, but if it somehow does, we'd rather keep serving requests than
    // take the whole process down.
    match registry.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Remove the worker sender for `model` from the registry (if present) and
/// return it. Dropping the returned sender causes the worker thread to wind
/// down once its current job is done.
fn evict_embedding_worker(model: &Path) -> Option<tokio::sync::mpsc::Sender<EmbeddingJob>> {
    let mut guard = lock_registry();
    let pos = guard.entries.iter().position(|(p, _)| p == model)?;
    let (_, tx) = guard.entries.remove(pos);
    tracing::info!(model = ?model, "Speaker embedding worker evicted");
    Some(tx)
}

fn embedding_worker(model: &Path) -> tokio::sync::mpsc::Sender<EmbeddingJob> {
    {
        let mut guard = lock_registry();
        if let Some(pos) = guard.entries.iter().position(|(p, _)| p == model) {
            // Touch: move to tail (most-recently-used).
            let entry = guard.entries.remove(pos);
            let tx = entry.1.clone();
            guard.entries.push(entry);
            return tx;
        }
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<EmbeddingJob>(EMBEDDING_WORKER_QUEUE);
    let model_path = model.to_path_buf();
    // A plain OS thread owns the !Send sherpa-onnx extractor for its whole
    // lifetime; tokio::spawn_blocking can't give us that guarantee.
    std::thread::spawn(move || {
        let config = sherpa_onnx::SpeakerEmbeddingExtractorConfig {
            model: Some(model_path.to_string_lossy().to_string()),
            num_threads: 1,
            provider: Some("cpu".to_string()),
            ..Default::default()
        };
        let extractor = match sherpa_onnx::SpeakerEmbeddingExtractor::create(&config) {
            Some(e) => e,
            None => {
                tracing::error!(
                    model = ?model_path,
                    "Failed to create SpeakerEmbeddingExtractor — evicting phantom worker"
                );
                // Drain any already-queued jobs with an error, then remove
                // ourselves from the registry so future callers re-attempt
                // creation instead of reusing this broken worker.
                while let Some((_, reply)) = rx.blocking_recv() {
                    let _ = reply.send(Err(anyhow::anyhow!(
                        "SpeakerEmbeddingExtractor create failed"
                    )));
                }
                let _ = evict_embedding_worker(&model_path);
                return;
            }
        };
        tracing::info!(model = ?model_path, "Speaker embedding worker ready");

        while let Some((audio, reply)) = rx.blocking_recv() {
            let result: anyhow::Result<Vec<f32>> = (|| {
                let stream = extractor.create_stream().ok_or_else(|| {
                    anyhow::anyhow!("SpeakerEmbeddingExtractor::create_stream returned None")
                })?;
                stream.accept_waveform(16000, &audio);
                stream.input_finished();
                if !extractor.is_ready(&stream) {
                    return Err(anyhow::anyhow!(
                        "SpeakerEmbeddingExtractor not ready after input_finished"
                    ));
                }
                extractor.compute(&stream).ok_or_else(|| {
                    anyhow::anyhow!("SpeakerEmbeddingExtractor::compute returned None")
                })
            })();
            let _ = reply.send(result);
        }
    });

    // Insert at tail and enforce LRU cap. Evicted senders are dropped here,
    // which signals their worker threads to exit when idle.
    let mut guard = lock_registry();
    guard.entries.push((model.to_path_buf(), tx.clone()));
    while guard.entries.len() > EMBEDDING_WORKER_CAP {
        let (evicted_path, _) = guard.entries.remove(0);
        tracing::info!(model = ?evicted_path, "Speaker embedding worker LRU-evicted");
    }
    tx
}

/// Extract a speaker embedding from audio. Routes to a cached worker thread
/// that owns a long-lived `SpeakerEmbeddingExtractor`, so repeat calls do
/// not re-parse the ONNX model.
pub async fn extract_embedding(
    embedding_model: &Path,
    audio: &[f32], // 16kHz mono f32
) -> anyhow::Result<SpeakerEmbedding> {
    let duration_samples = audio.len();
    // One retry: if the cached worker has exited (e.g. the channel was
    // evicted between `embedding_worker()` and `send`), drop the stale entry
    // and respawn. Without this, a single eviction races with an in-flight
    // call and produces a spurious "embedding worker closed" error.
    for attempt in 0..2 {
        let worker = embedding_worker(embedding_model);
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        match worker.send((audio.to_vec(), reply_tx)).await {
            Ok(()) => {
                let values = reply_rx
                    .await
                    .map_err(|_| anyhow::anyhow!("embedding worker dropped reply"))??;
                debug!(dimensions = values.len(), "Speaker embedding extracted");
                return Ok(SpeakerEmbedding {
                    values,
                    duration_samples,
                });
            }
            Err(_) => {
                // Worker was evicted or its thread exited. Evict the stale
                // entry and try once more with a fresh spawn.
                let _ = evict_embedding_worker(embedding_model);
                if attempt == 1 {
                    return Err(anyhow::anyhow!("embedding worker closed"));
                }
            }
        }
    }
    unreachable!();
}

/// Compute cosine similarity between two embedding vectors. Returns 0 when
/// the dimensions differ rather than panicking — dimension mismatches can
/// occur after a user swaps speaker-embedding models while stale rows from
/// the previous model still exist in the DB, and a panic inside the worker
/// thread would kill embedding processing for the rest of the session.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        tracing::warn!(
            a_len = a.len(),
            b_len = b.len(),
            "cosine_similarity dimension mismatch — returning 0.0"
        );
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a < 1e-8 || norm_b < 1e-8 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
