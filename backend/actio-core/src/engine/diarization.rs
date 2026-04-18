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

/// Extract a speaker embedding from audio. Used for enrollment.
/// Returns a 512-dimensional f32 vector.
pub async fn extract_embedding(
    embedding_model: &Path,
    audio: &[f32], // 16kHz mono f32
) -> anyhow::Result<SpeakerEmbedding> {
    let embedding_model = embedding_model.to_path_buf();
    let audio = audio.to_vec();
    let duration_samples = audio.len();

    tokio::task::spawn_blocking(move || {
        let config = sherpa_onnx::SpeakerEmbeddingExtractorConfig {
            model: Some(embedding_model.to_string_lossy().to_string()),
            num_threads: 1,
            provider: Some("cpu".to_string()),
            ..Default::default()
        };

        let extractor = match sherpa_onnx::SpeakerEmbeddingExtractor::create(&config) {
            Some(e) => e,
            None => {
                return Err(anyhow::anyhow!(
                    "Failed to create SpeakerEmbeddingExtractor — check model path"
                ));
            }
        };

        let stream = match extractor.create_stream() {
            Some(s) => s,
            None => {
                return Err(anyhow::anyhow!(
                    "SpeakerEmbeddingExtractor::create_stream returned None"
                ));
            }
        };

        stream.accept_waveform(16000, &audio);
        stream.input_finished();

        if !extractor.is_ready(&stream) {
            return Err(anyhow::anyhow!(
                "SpeakerEmbeddingExtractor not ready after input_finished"
            ));
        }

        let values = match extractor.compute(&stream) {
            Some(v) => v,
            None => {
                return Err(anyhow::anyhow!(
                    "SpeakerEmbeddingExtractor::compute returned None"
                ));
            }
        };

        debug!(dimensions = values.len(), "Speaker embedding extracted");

        Ok(SpeakerEmbedding {
            values,
            duration_samples,
        })
    })
    .await?
}

/// Compute cosine similarity between two embedding vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a < 1e-8 || norm_b < 1e-8 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
