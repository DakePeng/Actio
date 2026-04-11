use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// What subset of model files to download.
/// - `Shared`: files used by every model (VAD). Downloaded first.
/// - `Model(id)`: a specific ASR model pack (e.g. "zh_zipformer_14m",
///   "en_zipformer_20m", "moonshine_tiny_en", "sense_voice_multi",
///   "funasr_nano").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum DownloadTarget {
    Shared,
    Model(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelStatus {
    /// Shared files (VAD) not yet downloaded.
    NotDownloaded,
    Downloading {
        target: DownloadTarget,
        progress: f32,
        current_file: String,
    },
    /// Shared files are present. Language packs are tracked separately via
    /// `available_asr_models()`.
    Ready,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct AsrModelInfo {
    pub id: String,
    pub name: String,
    /// Human-readable comma-separated list, e.g. "Chinese", "English",
    /// "Chinese, English, Japanese, Korean, Cantonese".
    pub languages: String,
    pub size_mb: u32,
    pub ram_mb: u32,
    /// Short recommended CPU requirement, e.g. "Any CPU",
    /// "CPU with AVX2", "4+ cores recommended".
    pub recommended_cpu: String,
    /// True for real-time streaming ASR; false for offline (chunked) ASR.
    pub streaming: bool,
    pub description: String,
    /// Whether the model pack is fully present on disk.
    pub downloaded: bool,
    /// Whether the runtime pipeline currently supports loading this model.
    /// Downloadable models that aren't yet wired up can still appear in the
    /// catalog but won't be selectable until this flag flips to true.
    pub runtime_supported: bool,
}

#[derive(Debug, Clone)]
pub struct TransducerFiles {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SenseVoiceFiles {
    pub model: PathBuf,
    pub tokens: PathBuf,
}

/// Paraformer offline model files — used by the small Chinese+English
/// Paraformer, an order-of-magnitude smaller alternative to FunASR Nano.
#[derive(Debug, Clone)]
pub struct ParaformerFiles {
    pub model: PathBuf,
    pub tokens: PathBuf,
}

/// Moonshine v1 model files.
#[derive(Debug, Clone)]
pub struct MoonshineFiles {
    pub preprocessor: PathBuf,
    pub encoder: PathBuf,
    pub uncached_decoder: PathBuf,
    pub cached_decoder: PathBuf,
    pub tokens: PathBuf,
}

/// FunASR Nano (Qwen3-0.6B-backed) model files.
#[derive(Debug, Clone)]
pub struct FunAsrNanoFiles {
    pub encoder_adaptor: PathBuf,
    pub llm: PathBuf,
    pub embedding: PathBuf,
    /// Directory containing tokenizer.json / merges.txt / vocab.json.
    pub tokenizer_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ModelPaths {
    pub silero_vad: PathBuf,
    pub zh: Option<TransducerFiles>,
    pub en: Option<TransducerFiles>,
    pub ko: Option<TransducerFiles>,
    pub sense_voice: Option<SenseVoiceFiles>,
    pub moonshine_en: Option<MoonshineFiles>,
    pub paraformer_zh_small: Option<ParaformerFiles>,
    pub funasr_nano: Option<FunAsrNanoFiles>,
    /// Reserved for future speaker-diarization download target.
    pub pyannote_segmentation: Option<PathBuf>,
    /// Reserved for future speaker-embedding download target.
    pub speaker_embedding: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Internal model file descriptors
// ---------------------------------------------------------------------------

struct ModelFile {
    /// URL to download from
    url: &'static str,
    /// Filename to store as inside model_dir
    dest_name: &'static str,
    /// Whether this is a tar.bz2 archive that needs extraction
    extract_inner: Option<&'static str>,
}

/// Files shared by every language — downloaded first.
const SHARED_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
        dest_name: "silero_vad.onnx",
        extract_inner: None,
    },
];

/// Chinese Zipformer streaming transducer 14M.
const ZH_ZIPFORMER_14M_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        dest_name: "zh_encoder.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        dest_name: "zh_decoder.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        dest_name: "zh_joiner.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/tokens.txt",
        dest_name: "zh_tokens.txt",
        extract_inner: None,
    },
];

/// English Zipformer streaming transducer 20M.
const EN_ZIPFORMER_20M_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        dest_name: "en_encoder.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        dest_name: "en_decoder.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        dest_name: "en_joiner.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/tokens.txt",
        dest_name: "en_tokens.txt",
        extract_inner: None,
    },
];

/// Korean streaming Zipformer — ~133 MB total (encoder 127 MB int8 dominates).
/// Source: https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16
const KO_ZIPFORMER_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        dest_name: "ko_encoder.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        dest_name: "ko_decoder.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        dest_name: "ko_joiner.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/tokens.txt",
        dest_name: "ko_tokens.txt",
        extract_inner: None,
    },
];

/// Paraformer Chinese-small — offline bilingual zh+en, ~82 MB total.
/// Much smaller alternative to FunASR Nano (~1 GB).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09
const PARAFORMER_ZH_SMALL_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/model.int8.onnx",
        dest_name: "paraformer_zh_small.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/tokens.txt",
        dest_name: "paraformer_zh_small_tokens.txt",
        extract_inner: None,
    },
];

/// Moonshine Tiny English — offline ASR, int8 quantised (~125 MB total).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8
const MOONSHINE_TINY_EN_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/preprocess.onnx",
        dest_name: "moonshine_tiny_en_preprocess.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/encode.int8.onnx",
        dest_name: "moonshine_tiny_en_encode.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/uncached_decode.int8.onnx",
        dest_name: "moonshine_tiny_en_uncached_decode.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/cached_decode.int8.onnx",
        dest_name: "moonshine_tiny_en_cached_decode.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/tokens.txt",
        dest_name: "moonshine_tiny_en_tokens.txt",
        extract_inner: None,
    },
];

/// SenseVoice multilingual (zh/en/ja/ko/yue) — offline int8 (~239 MB).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17
const SENSE_VOICE_MULTI_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx",
        dest_name: "sense_voice_multi.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt",
        dest_name: "sense_voice_multi_tokens.txt",
        extract_inner: None,
    },
];

/// FunASR Nano — Qwen3-0.6B-backed LLM ASR, int8 (~1 GB total).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30
///
/// The tokenizer files live under `funasr_nano_tokenizer/` because
/// sherpa-onnx's `OfflineFunASRNanoModelConfig.tokenizer` takes a directory
/// path, not a file path.
const FUNASR_NANO_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/embedding.int8.onnx",
        dest_name: "funasr_nano_embedding.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/encoder_adaptor.int8.onnx",
        dest_name: "funasr_nano_encoder_adaptor.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/llm.int8.onnx",
        dest_name: "funasr_nano_llm.int8.onnx",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/Qwen3-0.6B/tokenizer.json",
        dest_name: "funasr_nano_tokenizer/tokenizer.json",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/Qwen3-0.6B/merges.txt",
        dest_name: "funasr_nano_tokenizer/merges.txt",
        extract_inner: None,
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/Qwen3-0.6B/vocab.json",
        dest_name: "funasr_nano_tokenizer/vocab.json",
        extract_inner: None,
    },
];

fn files_for_target(target: &DownloadTarget) -> Result<&'static [ModelFile]> {
    match target {
        DownloadTarget::Shared => Ok(SHARED_FILES),
        DownloadTarget::Model(id) => match id.as_str() {
            "zh_zipformer_14m" => Ok(ZH_ZIPFORMER_14M_FILES),
            "en_zipformer_20m" => Ok(EN_ZIPFORMER_20M_FILES),
            "ko_zipformer" => Ok(KO_ZIPFORMER_FILES),
            "paraformer_zh_small" => Ok(PARAFORMER_ZH_SMALL_FILES),
            "moonshine_tiny_en" => Ok(MOONSHINE_TINY_EN_FILES),
            "sense_voice_multi" => Ok(SENSE_VOICE_MULTI_FILES),
            "funasr_nano" => Ok(FUNASR_NANO_FILES),
            other => Err(anyhow!("Unknown model pack: {}", other)),
        },
    }
}

/// Check whether all files for a given model id are present on disk.
fn model_downloaded(model_dir: &PathBuf, files: &[ModelFile]) -> bool {
    files.iter().all(|f| {
        let p = model_dir.join(f.dest_name);
        p.exists() && std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------
// ModelManager
// ---------------------------------------------------------------------------

pub struct ModelManager {
    model_dir: PathBuf,
    status: Arc<RwLock<ModelStatus>>,
}

impl ModelManager {
    /// Create a new ModelManager. Checks for existing model files and sets
    /// status to Ready if shared files are present, otherwise NotDownloaded.
    pub fn new(model_dir: PathBuf) -> Self {
        let status = detect_existing_status(&model_dir);
        info!(?status, model_dir = %model_dir.display(), "ModelManager initialised");
        Self {
            model_dir,
            status: Arc::new(RwLock::new(status)),
        }
    }

    /// Return the current status (cloned).
    pub async fn status(&self) -> ModelStatus {
        self.status.read().await.clone()
    }

    /// Return info about available ASR models and their download status.
    /// The catalog is static — only the `downloaded` flag is computed per call.
    pub fn available_asr_models(&self) -> Vec<AsrModelInfo> {
        let d = &self.model_dir;
        vec![
            AsrModelInfo {
                id: "zh_zipformer_14m".to_string(),
                name: "Zipformer 14M (Chinese)".to_string(),
                languages: "Chinese".to_string(),
                size_mb: 25,
                ram_mb: 200,
                recommended_cpu: "Any modern CPU".to_string(),
                streaming: true,
                description:
                    "Real-time streaming Chinese ASR. 14 million parameters, \
                     int8-quantised. Very low latency; suitable for always-on \
                     transcription on laptops."
                        .to_string(),
                downloaded: model_downloaded(d, ZH_ZIPFORMER_14M_FILES),
                runtime_supported: true,
            },
            AsrModelInfo {
                id: "en_zipformer_20m".to_string(),
                name: "Zipformer 20M (English)".to_string(),
                languages: "English".to_string(),
                size_mb: 44,
                ram_mb: 250,
                recommended_cpu: "Any modern CPU".to_string(),
                streaming: true,
                description:
                    "Real-time streaming English ASR. 20 million parameters, \
                     int8-quantised. Very low latency; suitable for always-on \
                     transcription on laptops."
                        .to_string(),
                downloaded: model_downloaded(d, EN_ZIPFORMER_20M_FILES),
                runtime_supported: true,
            },
            AsrModelInfo {
                id: "ko_zipformer".to_string(),
                name: "Zipformer (Korean)".to_string(),
                languages: "Korean".to_string(),
                size_mb: 133,
                ram_mb: 500,
                recommended_cpu: "CPU with AVX2".to_string(),
                streaming: true,
                description:
                    "Real-time streaming Korean ASR. int8-quantised Zipformer \
                     transducer. Suitable for live transcription on modern \
                     laptops."
                        .to_string(),
                downloaded: model_downloaded(d, KO_ZIPFORMER_FILES),
                runtime_supported: true,
            },
            AsrModelInfo {
                id: "paraformer_zh_small".to_string(),
                name: "Paraformer Small (Chinese + English)".to_string(),
                languages: "Chinese, English".to_string(),
                size_mb: 82,
                ram_mb: 400,
                recommended_cpu: "Any modern CPU".to_string(),
                streaming: false,
                description:
                    "Offline bilingual ASR from FunAudioLLM. Non-autoregressive \
                     Paraformer architecture — much smaller than FunASR Nano \
                     (~82 MB vs ~1 GB) while still covering Mandarin and \
                     English. Processes complete utterances via VAD."
                        .to_string(),
                downloaded: model_downloaded(d, PARAFORMER_ZH_SMALL_FILES),
                runtime_supported: true,
            },
            AsrModelInfo {
                id: "moonshine_tiny_en".to_string(),
                name: "Moonshine Tiny (English)".to_string(),
                languages: "English".to_string(),
                size_mb: 125,
                ram_mb: 500,
                recommended_cpu: "CPU with AVX2".to_string(),
                streaming: false,
                description:
                    "Offline English ASR from Useful Sensors. ~27 million \
                     parameters, int8-quantised. Higher accuracy than the \
                     Zipformer streaming models; processes complete utterances \
                     via VAD rather than streaming."
                        .to_string(),
                downloaded: model_downloaded(d, MOONSHINE_TINY_EN_FILES),
                runtime_supported: true,
            },
            AsrModelInfo {
                id: "sense_voice_multi".to_string(),
                name: "SenseVoice Multilingual".to_string(),
                languages: "Chinese, English, Japanese, Korean, Cantonese"
                    .to_string(),
                size_mb: 239,
                ram_mb: 800,
                recommended_cpu: "CPU with AVX2, 4+ cores recommended".to_string(),
                streaming: false,
                description:
                    "Offline multilingual ASR from Alibaba FunAudioLLM. ~234 \
                     million parameters, int8-quantised. Auto language \
                     detection across 5 languages; high accuracy but \
                     processes complete utterances via VAD, not streaming."
                        .to_string(),
                downloaded: model_downloaded(d, SENSE_VOICE_MULTI_FILES),
                runtime_supported: true,
            },
            AsrModelInfo {
                id: "funasr_nano".to_string(),
                name: "FunASR Nano (Qwen3 0.6B)".to_string(),
                languages: "Chinese, English".to_string(),
                size_mb: 1010,
                ram_mb: 2000,
                recommended_cpu: "CPU with AVX2, 8+ cores recommended".to_string(),
                streaming: false,
                description:
                    "LLM-powered ASR that feeds encoded audio into Qwen3-0.6B \
                     (~600 million parameter LLM) as the decoder. Highest \
                     quality of the catalog but each segment runs a full LLM \
                     forward pass — expect multi-second latency on CPU. Not \
                     streaming."
                        .to_string(),
                downloaded: model_downloaded(d, FUNASR_NANO_FILES),
                runtime_supported: true,
            },
        ]
    }

    /// Return paths to model files if the shared tier is present. Callers must
    /// still verify that at least one language pack is downloaded before
    /// attempting to start ASR.
    pub async fn model_paths(&self) -> Option<ModelPaths> {
        let status = self.status.read().await;
        match &*status {
            ModelStatus::Ready => Some(build_paths(&self.model_dir)),
            _ => None,
        }
    }

    /// Delete all files for the given target id and return the number of
    /// files removed. Accepts either a model id (e.g. "zh_zipformer_14m",
    /// "paraformer_zh_small") or the literal string "shared" for the Silero
    /// VAD tier.
    ///
    /// After removing files, also removes any now-empty subdirectories (e.g.
    /// the `funasr_nano_tokenizer/` dir) and re-derives the shared-tier
    /// status if the shared files were touched.
    pub async fn delete_model(&self, id: &str) -> Result<u32> {
        let files: &'static [ModelFile] = if id == "shared" {
            SHARED_FILES
        } else {
            files_for_target(&DownloadTarget::Model(id.to_string()))?
        };

        // Guard: don't delete while a download is in flight — files could
        // be half-written and we'd race with the downloader.
        {
            let s = self.status.read().await;
            if matches!(&*s, ModelStatus::Downloading { .. }) {
                return Err(anyhow!(
                    "Cannot delete while a download is in progress"
                ));
            }
        }

        let mut count: u32 = 0;
        let mut subdirs: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();

        for f in files {
            let p = self.model_dir.join(f.dest_name);
            if let Some(parent) = p.parent() {
                if parent != self.model_dir {
                    subdirs.insert(parent.to_path_buf());
                }
            }
            if p.exists() {
                tokio::fs::remove_file(&p)
                    .await
                    .with_context(|| format!("removing {}", p.display()))?;
                count += 1;
            }
        }

        // Clean up empty subdirectories (only the ones we just emptied).
        for dir in &subdirs {
            if !dir.exists() {
                continue;
            }
            // Try to remove; if it's still non-empty this errors harmlessly.
            let _ = tokio::fs::remove_dir(dir).await;
        }

        // Re-derive status from disk so the Ready flag matches reality.
        let new_status = detect_existing_status(&self.model_dir);
        *self.status.write().await = new_status;

        info!(model = %id, deleted = count, "Model files deleted");
        Ok(count)
    }

    /// Return the absolute paths that should be pre-read to warm the OS file
    /// cache for the given ASR model id. Includes Silero VAD when the model
    /// is an offline recognizer (SenseVoice / Moonshine / FunASR Nano).
    pub fn files_to_warmup(&self, asr_model: &str) -> Result<Vec<PathBuf>> {
        let dir = &self.model_dir;
        let join = |name: &str| dir.join(name);
        let collect = |files: &[ModelFile]| -> Vec<PathBuf> {
            files.iter().map(|f| dir.join(f.dest_name)).collect()
        };

        let (mut paths, needs_vad) = match asr_model {
            "zh_zipformer_14m" => (collect(ZH_ZIPFORMER_14M_FILES), false),
            "en_zipformer_20m" => (collect(EN_ZIPFORMER_20M_FILES), false),
            "ko_zipformer" => (collect(KO_ZIPFORMER_FILES), false),
            "paraformer_zh_small" => (collect(PARAFORMER_ZH_SMALL_FILES), true),
            "sense_voice_multi" => (collect(SENSE_VOICE_MULTI_FILES), true),
            "moonshine_tiny_en" => (collect(MOONSHINE_TINY_EN_FILES), true),
            "funasr_nano" => (collect(FUNASR_NANO_FILES), true),
            other => return Err(anyhow!("Unknown ASR model: {}", other)),
        };
        if needs_vad {
            paths.insert(0, join("silero_vad.onnx"));
        }
        Ok(paths)
    }

    /// Start downloading the given target in a background task. Progress is
    /// sent via the watch channel. The task updates the shared status on
    /// completion or error.
    pub async fn start_download(
        &self,
        target: DownloadTarget,
        progress_tx: tokio::sync::watch::Sender<ModelStatus>,
    ) -> Result<()> {
        // Validate target early so we can return an API error instead of
        // failing silently inside the background task.
        files_for_target(&target)?;

        // Guard: already downloading
        {
            let s = self.status.read().await;
            if matches!(&*s, ModelStatus::Downloading { .. }) {
                return Err(anyhow!("Download already in progress"));
            }
        }

        let model_dir = self.model_dir.clone();
        let status_arc = Arc::clone(&self.status);
        let target_clone = target.clone();

        tokio::fs::create_dir_all(&model_dir)
            .await
            .with_context(|| format!("creating model_dir {}", model_dir.display()))?;

        tokio::spawn(async move {
            let result =
                download_target(&model_dir, &target_clone, &status_arc, &progress_tx).await;

            // After any download attempt, re-derive status from on-disk state
            // so language-pack downloads don't overwrite the top-level Ready
            // flag and a failed language download still leaves shared intact.
            let final_status = match result {
                Ok(()) => detect_existing_status(&model_dir),
                Err(e) => {
                    warn!(error = %e, "Model download failed");
                    ModelStatus::Error {
                        message: e.to_string(),
                    }
                }
            };

            let _ = progress_tx.send(final_status.clone());
            *status_arc.write().await = final_status;
        });

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Detect whether the shared files are already downloaded.
fn detect_existing_status(model_dir: &PathBuf) -> ModelStatus {
    let shared_ok = SHARED_FILES
        .iter()
        .all(|f| model_dir.join(f.dest_name).exists());

    if shared_ok {
        ModelStatus::Ready
    } else {
        ModelStatus::NotDownloaded
    }
}

fn build_transducer(model_dir: &PathBuf, prefix: &str) -> Option<TransducerFiles> {
    let p = |suffix: &str| model_dir.join(format!("{prefix}_{suffix}"));
    let enc = p("encoder.int8.onnx");
    let dec = p("decoder.int8.onnx");
    let join = p("joiner.int8.onnx");
    let tok = p("tokens.txt");
    if enc.exists() && dec.exists() && join.exists() && tok.exists() {
        Some(TransducerFiles { encoder: enc, decoder: dec, joiner: join, tokens: tok })
    } else {
        None
    }
}

fn build_paths(model_dir: &PathBuf) -> ModelPaths {
    let opt = |name: &str| {
        let path = model_dir.join(name);
        if path.exists() { Some(path) } else { None }
    };

    let sense_voice = {
        let model = model_dir.join("sense_voice_multi.int8.onnx");
        let tokens = model_dir.join("sense_voice_multi_tokens.txt");
        if model.exists() && tokens.exists() {
            Some(SenseVoiceFiles { model, tokens })
        } else {
            None
        }
    };

    let moonshine_en = {
        let preprocessor = model_dir.join("moonshine_tiny_en_preprocess.onnx");
        let encoder = model_dir.join("moonshine_tiny_en_encode.int8.onnx");
        let uncached_decoder = model_dir.join("moonshine_tiny_en_uncached_decode.int8.onnx");
        let cached_decoder = model_dir.join("moonshine_tiny_en_cached_decode.int8.onnx");
        let tokens = model_dir.join("moonshine_tiny_en_tokens.txt");
        if preprocessor.exists()
            && encoder.exists()
            && uncached_decoder.exists()
            && cached_decoder.exists()
            && tokens.exists()
        {
            Some(MoonshineFiles {
                preprocessor,
                encoder,
                uncached_decoder,
                cached_decoder,
                tokens,
            })
        } else {
            None
        }
    };

    let funasr_nano = {
        let encoder_adaptor = model_dir.join("funasr_nano_encoder_adaptor.int8.onnx");
        let llm = model_dir.join("funasr_nano_llm.int8.onnx");
        let embedding = model_dir.join("funasr_nano_embedding.int8.onnx");
        let tokenizer_dir = model_dir.join("funasr_nano_tokenizer");
        let tokenizer_json = tokenizer_dir.join("tokenizer.json");
        let merges = tokenizer_dir.join("merges.txt");
        let vocab = tokenizer_dir.join("vocab.json");
        if encoder_adaptor.exists()
            && llm.exists()
            && embedding.exists()
            && tokenizer_json.exists()
            && merges.exists()
            && vocab.exists()
        {
            Some(FunAsrNanoFiles {
                encoder_adaptor,
                llm,
                embedding,
                tokenizer_dir,
            })
        } else {
            None
        }
    };

    let paraformer_zh_small = {
        let model = model_dir.join("paraformer_zh_small.int8.onnx");
        let tokens = model_dir.join("paraformer_zh_small_tokens.txt");
        if model.exists() && tokens.exists() {
            Some(ParaformerFiles { model, tokens })
        } else {
            None
        }
    };

    ModelPaths {
        silero_vad: model_dir.join("silero_vad.onnx"),
        zh: build_transducer(model_dir, "zh"),
        en: build_transducer(model_dir, "en"),
        ko: build_transducer(model_dir, "ko"),
        sense_voice,
        moonshine_en,
        paraformer_zh_small,
        funasr_nano,
        pyannote_segmentation: opt("pyannote-seg3.onnx"),
        speaker_embedding: opt("speaker_eres2net.onnx"),
    }
}

/// Download all files for a target, updating status along the way.
async fn download_target(
    model_dir: &PathBuf,
    target: &DownloadTarget,
    status_arc: &Arc<RwLock<ModelStatus>>,
    progress_tx: &tokio::sync::watch::Sender<ModelStatus>,
) -> Result<()> {
    let files = files_for_target(target)?;
    let total = files.len() as f32;
    let client = reqwest::Client::new();

    for (idx, file) in files.iter().enumerate() {
        let progress = idx as f32 / total;
        let current_status = ModelStatus::Downloading {
            target: target.clone(),
            progress,
            current_file: file.dest_name.to_string(),
        };
        *status_arc.write().await = current_status.clone();
        let _ = progress_tx.send(current_status);

        download_file_with_retry(&client, file, model_dir).await?;

        info!(
            file = file.dest_name,
            progress = (idx + 1) as f32 / total,
            "Model file ready"
        );
    }

    Ok(())
}

async fn download_file_with_retry(
    client: &reqwest::Client,
    file: &ModelFile,
    model_dir: &PathBuf,
) -> Result<()> {
    let dest = model_dir.join(file.dest_name);

    // Skip if file already exists with non-zero size
    if dest.exists() {
        let meta = std::fs::metadata(&dest)?;
        if meta.len() > 0 {
            info!(file = file.dest_name, "Skipping already-downloaded file");
            return Ok(());
        }
    }

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=3u8 {
        match do_download(client, file, model_dir, &dest).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!(attempt, file = file.dest_name, error = %e, "Download attempt failed");
                last_err = Some(e);
                // small backoff
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("Download failed after 3 attempts")))
}

async fn do_download(
    client: &reqwest::Client,
    file: &ModelFile,
    _model_dir: &PathBuf,
    dest: &PathBuf,
) -> Result<()> {
    info!(url = file.url, dest = %dest.display(), "Downloading model file");

    // Ensure parent directory exists (dest_name may include subdirs,
    // e.g. "funasr_nano_tokenizer/tokenizer.json").
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating parent dir {}", parent.display()))?;
    }

    let response = client
        .get(file.url)
        .send()
        .await
        .with_context(|| format!("GET {}", file.url))?
        .error_for_status()
        .with_context(|| format!("HTTP error for {}", file.url))?;

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("reading response body from {}", file.url))?;

    if let Some(inner_path) = file.extract_inner {
        // It's a tar.bz2 — extract the inner file
        extract_bz2_tar(&bytes, inner_path, dest)
            .with_context(|| format!("extracting {} from archive", inner_path))?;
    } else {
        tokio::fs::write(dest, &bytes)
            .await
            .with_context(|| format!("writing {}", dest.display()))?;
    }

    // Verify non-zero size
    let meta = std::fs::metadata(dest)?;
    if meta.len() == 0 {
        return Err(anyhow!("Downloaded file {} has zero size", dest.display()));
    }

    Ok(())
}

/// Extract a single named file from a tar.bz2 in-memory blob.
#[allow(dead_code)]
fn extract_bz2_tar(data: &[u8], inner_name: &str, dest: &PathBuf) -> Result<()> {
    use bzip2::read::BzDecoder;
    use tar::Archive;

    let bz = BzDecoder::new(data);
    let mut archive = Archive::new(bz);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        // Match filename (any directory prefix is ignored)
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if file_name == inner_name {
            let mut out = std::fs::File::create(dest)
                .with_context(|| format!("creating {}", dest.display()))?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }

    Err(anyhow!(
        "Could not find '{}' inside tar.bz2 archive",
        inner_name
    ))
}
