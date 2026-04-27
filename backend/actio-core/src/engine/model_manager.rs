use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::engine::llm_catalog::DownloadSource;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// What subset of model files to download.
/// - `Shared`: files used by every model (VAD). Downloaded first.
/// - `Model(id)`: a specific ASR model pack (e.g. "zh_zipformer_14m",
///   "en_zipformer_20m", "moonshine_tiny_en", "sense_voice_multi",
///   "funasr_nano").
/// - `Embedding(id)`: a specific speaker-embedding model from the catalog
///   (e.g. "campplus_zh_en", "eres2netv2", "titanet_small_en").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, utoipa::ToSchema)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum DownloadTarget {
    Shared,
    Model(String),
    Embedding(String),
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

/// Speaker-embedding model surfaced to the Settings UI.
#[derive(Debug, Clone, Serialize)]
pub struct SpeakerEmbeddingModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub languages: String,
    pub size_mb: u32,
    pub embedding_dim: u32,
    pub downloaded: bool,
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

/// Zipformer CTC offline model files — model + tokens + optional BPE vocab.
#[derive(Debug, Clone)]
pub struct ZipformerCtcFiles {
    pub model: PathBuf,
    pub tokens: PathBuf,
    pub bpe_vocab: Option<PathBuf>,
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

/// Whisper model files (encoder + decoder + tokens).
#[derive(Debug, Clone)]
pub struct WhisperFiles {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub tokens: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ModelPaths {
    pub silero_vad: PathBuf,
    /// All streaming transducer models keyed by their catalog id.
    pub transducers: std::collections::HashMap<String, TransducerFiles>,
    pub sense_voice: Option<SenseVoiceFiles>,
    pub moonshine_en: Option<MoonshineFiles>,
    pub paraformer_zh_small: Option<ParaformerFiles>,
    pub zipformer_ctc_zh_small: Option<ZipformerCtcFiles>,
    pub funasr_nano: Option<FunAsrNanoFiles>,
    pub whisper_base: Option<WhisperFiles>,
    pub whisper_turbo: Option<WhisperFiles>,
    pub pyannote_segmentation: Option<PathBuf>,
    pub speaker_embedding: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Internal model file descriptors
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct ModelFile {
    url: &'static str,
    dest_name: &'static str,
}

/// Files shared by every language — downloaded first.
const SHARED_FILES: &[ModelFile] = &[ModelFile {
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
    dest_name: "silero_vad.onnx",
}];

const SILERO_VAD_BYTES: &[u8] = include_bytes!("../../assets/silero_vad.onnx");

// ---------------------------------------------------------------------------
// Streaming transducer table — every streaming model is defined here.
// Adding a new one is just a new row. The download, catalog, build, and
// pipeline routing code all iterate this table automatically.
// ---------------------------------------------------------------------------

struct StreamingTransducerDef {
    id: &'static str,
    name: &'static str,
    languages: &'static str,
    size_mb: u32,
    ram_mb: u32,
    cpu: &'static str,
    description: &'static str,
    /// Dest-name prefix. Files are stored as {prefix}_encoder.int8.onnx, etc.
    prefix: &'static str,
    encoder_url: &'static str,
    decoder_url: &'static str,
    joiner_url: &'static str,
    tokens_url: &'static str,
}

const STREAMING_TRANSDUCERS: &[StreamingTransducerDef] = &[
    // ── Single-language: Chinese ─────────────────────────────────────
    StreamingTransducerDef {
        id: "zh_zipformer_14m",
        name: "Zipformer 14M (Chinese)",
        languages: "Chinese",
        size_mb: 25,
        ram_mb: 200,
        cpu: "Any modern CPU",
        description: "Real-time streaming Chinese ASR. 14M params, int8. Very low latency.",
        prefix: "zh",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-zh-14M-2023-02-23/resolve/main/tokens.txt",
    },
    StreamingTransducerDef {
        id: "zh_conformer",
        name: "Conformer (Chinese)",
        languages: "Chinese",
        size_mb: 183,
        ram_mb: 600,
        cpu: "CPU with AVX2",
        description: "Streaming Chinese ASR using Conformer architecture. Higher accuracy than Zipformer 14M but larger.",
        prefix: "zh_conf",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-conformer-zh-2023-05-23/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-conformer-zh-2023-05-23/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-conformer-zh-2023-05-23/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-conformer-zh-2023-05-23/resolve/main/tokens.txt",
    },
    StreamingTransducerDef {
        id: "zh_lstm",
        name: "LSTM (Chinese)",
        languages: "Chinese",
        size_mb: 99,
        ram_mb: 400,
        cpu: "Any modern CPU",
        description: "Streaming Chinese ASR using LSTM-transducer. Lighter than Conformer, good balance of size and accuracy.",
        prefix: "zh_lstm",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-zh-2023-02-20/resolve/main/encoder-epoch-11-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-zh-2023-02-20/resolve/main/decoder-epoch-11-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-zh-2023-02-20/resolve/main/joiner-epoch-11-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-zh-2023-02-20/resolve/main/tokens.txt",
    },
    // ── Single-language: English ─────────────────────────────────────
    StreamingTransducerDef {
        id: "en_zipformer_20m",
        name: "Zipformer 20M (English)",
        languages: "English",
        size_mb: 44,
        ram_mb: 250,
        cpu: "Any modern CPU",
        description: "Real-time streaming English ASR. 20M params, int8. Very low latency.",
        prefix: "en",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/resolve/main/tokens.txt",
    },
    StreamingTransducerDef {
        id: "en_zipformer",
        name: "Zipformer (English)",
        languages: "English",
        size_mb: 73,
        ram_mb: 350,
        cpu: "Any modern CPU",
        description: "Streaming English Zipformer transducer. Higher accuracy than the 20M variant.",
        prefix: "en_zip",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26/resolve/main/encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26/resolve/main/decoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26/resolve/main/joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-26/resolve/main/tokens.txt",
    },
    StreamingTransducerDef {
        id: "en_zipformer_large",
        name: "Zipformer Large (English)",
        languages: "English",
        size_mb: 189,
        ram_mb: 600,
        cpu: "CPU with AVX2",
        description: "Large streaming English Zipformer. Best streaming English accuracy but heavy.",
        prefix: "en_lg",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-21/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-21/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-21/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-06-21/resolve/main/tokens.txt",
    },
    StreamingTransducerDef {
        id: "en_zipformer_medium",
        name: "Zipformer Medium (English)",
        languages: "English",
        size_mb: 128,
        ram_mb: 450,
        cpu: "Any modern CPU",
        description: "Medium-sized streaming English Zipformer. Good accuracy/size balance.",
        prefix: "en_md",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-02-21/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-02-21/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-02-21/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-en-2023-02-21/resolve/main/tokens.txt",
    },
    StreamingTransducerDef {
        id: "en_lstm",
        name: "LSTM (English)",
        languages: "English",
        size_mb: 86,
        ram_mb: 350,
        cpu: "Any modern CPU",
        description: "Streaming English LSTM-transducer. Compact and efficient.",
        prefix: "en_lstm",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-en-2023-02-17/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-en-2023-02-17/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-en-2023-02-17/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-lstm-en-2023-02-17/resolve/main/tokens.txt",
    },
    // ── Single-language: Korean ──────────────────────────────────────
    StreamingTransducerDef {
        id: "ko_zipformer",
        name: "Zipformer (Korean)",
        languages: "Korean",
        size_mb: 133,
        ram_mb: 500,
        cpu: "CPU with AVX2",
        description: "Real-time streaming Korean ASR. int8 Zipformer transducer.",
        prefix: "ko",
        encoder_url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/k2-fsa/sherpa-onnx-streaming-zipformer-korean-2024-06-16/resolve/main/tokens.txt",
    },
    // ── Single-language: French ──────────────────────────────────────
    StreamingTransducerDef {
        id: "fr_zipformer",
        name: "Zipformer (French)",
        languages: "French",
        size_mb: 129,
        ram_mb: 450,
        cpu: "CPU with AVX2",
        description: "Streaming French ASR. int8 Zipformer transducer.",
        prefix: "fr",
        encoder_url: "https://huggingface.co/shaojieli/sherpa-onnx-streaming-zipformer-fr-2023-04-14/resolve/main/encoder-epoch-29-avg-9-with-averaged-model.int8.onnx",
        decoder_url: "https://huggingface.co/shaojieli/sherpa-onnx-streaming-zipformer-fr-2023-04-14/resolve/main/decoder-epoch-29-avg-9-with-averaged-model.int8.onnx",
        joiner_url: "https://huggingface.co/shaojieli/sherpa-onnx-streaming-zipformer-fr-2023-04-14/resolve/main/joiner-epoch-29-avg-9-with-averaged-model.int8.onnx",
        tokens_url: "https://huggingface.co/shaojieli/sherpa-onnx-streaming-zipformer-fr-2023-04-14/resolve/main/tokens.txt",
    },
    // ── Multi-language ───────────────────────────────────────────────
    StreamingTransducerDef {
        id: "zhen_zipformer_bilingual",
        name: "Zipformer Bilingual (Chinese + English)",
        languages: "Chinese, English",
        size_mb: 198,
        ram_mb: 650,
        cpu: "CPU with AVX2",
        description: "Streaming bilingual Chinese+English ASR in a single model. No language switching needed.",
        prefix: "zhen",
        encoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/main/encoder-epoch-99-avg-1.int8.onnx",
        decoder_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/main/decoder-epoch-99-avg-1.int8.onnx",
        joiner_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/main/joiner-epoch-99-avg-1.int8.onnx",
        tokens_url: "https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-bilingual-zh-en-2023-02-20/resolve/main/tokens.txt",
    },
];

/// Build the 4 download ModelFile descriptors for a streaming transducer.
fn transducer_model_files(def: &StreamingTransducerDef) -> Vec<ModelFile> {
    vec![
        ModelFile {
            url: def.encoder_url,
            dest_name: Box::leak(format!("{}_encoder.int8.onnx", def.prefix).into_boxed_str()),
        },
        ModelFile {
            url: def.decoder_url,
            dest_name: Box::leak(format!("{}_decoder.int8.onnx", def.prefix).into_boxed_str()),
        },
        ModelFile {
            url: def.joiner_url,
            dest_name: Box::leak(format!("{}_joiner.int8.onnx", def.prefix).into_boxed_str()),
        },
        ModelFile {
            url: def.tokens_url,
            dest_name: Box::leak(format!("{}_tokens.txt", def.prefix).into_boxed_str()),
        },
    ]
}

/// Look up a streaming transducer definition by model id.
fn find_transducer(id: &str) -> Option<&'static StreamingTransducerDef> {
    STREAMING_TRANSDUCERS.iter().find(|d| d.id == id)
}

// ─────────────────────────────────────────────────────────────────────────────
// Speaker-embedding model catalog
// ─────────────────────────────────────────────────────────────────────────────
//
// Users pick one active model in Settings → Common Models. Switching models
// invalidates existing voiceprints because embeddings from different models
// live in different spaces (and different dimensions). The DB already tracks
// `embedding_dimension` per row so cross-model rows are silently ignored at
// query time; the UI surfaces a confirm dialog on switch.
//
// NOTE: the upstream release tag `speaker-recongition-models` is spelled
// with a typo (missing 'g'). Keep it — it is the canonical URL.

struct SpeakerEmbeddingModelDef {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    languages: &'static str,
    size_mb: u32,
    dim: u32,
    url: &'static str,
    dest_name: &'static str,
}

const SPEAKER_EMBEDDING_MODELS: &[SpeakerEmbeddingModelDef] = &[
    SpeakerEmbeddingModelDef {
        id: "campplus_zh_en",
        name: "CAM++ Advanced (Chinese + English)",
        description: "Context-aware masking, bilingual. Small, fast, well-balanced. Recommended default.",
        languages: "Chinese, English",
        size_mb: 27,
        dim: 192,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
        dest_name: "speaker_campplus_zh_en.onnx",
    },
    SpeakerEmbeddingModelDef {
        id: "campplus_zh",
        name: "CAM++ (Chinese)",
        description: "Chinese-optimised CAM++. Slightly crisper on native zh speech than the bilingual variant.",
        languages: "Chinese",
        size_mb: 27,
        dim: 192,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx",
        dest_name: "speaker_campplus_zh.onnx",
    },
    SpeakerEmbeddingModelDef {
        id: "campplus_en",
        name: "CAM++ (English)",
        description: "English-only CAM++ trained on VoxCeleb. Use for English-only scenarios where you want CAM++'s speed.",
        languages: "English",
        size_mb: 27,
        dim: 192,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
        dest_name: "speaker_campplus_en.onnx",
    },
    SpeakerEmbeddingModelDef {
        id: "eres2net_base",
        name: "ERes2Net Base (Chinese)",
        description: "Multi-scale Res2Net blocks. More accurate than CAM++ on varied audio, slightly slower. 512-dim embeddings (4x per-voiceprint storage).",
        languages: "Chinese",
        size_mb: 38,
        dim: 512,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_base_200k_sv_zh-cn_16k-common.onnx",
        dest_name: "speaker_eres2net_base.onnx",
    },
    SpeakerEmbeddingModelDef {
        id: "eres2netv2",
        name: "ERes2Net v2 (Chinese)",
        description: "Newer ERes2Net architecture. Best accuracy in this set; largest file.",
        languages: "Chinese",
        size_mb: 68,
        dim: 192,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2netv2_sv_zh-cn_16k-common.onnx",
        dest_name: "speaker_eres2netv2.onnx",
    },
    SpeakerEmbeddingModelDef {
        id: "titanet_small_en",
        name: "NeMo TitaNet Small (English)",
        description: "NVIDIA NeMo, English-tuned. Different architecture than CAM++/ERes2Net; useful if you want a non-3D-Speaker alternative.",
        languages: "English",
        size_mb: 38,
        dim: 192,
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/nemo_en_titanet_small.onnx",
        dest_name: "speaker_titanet_small.onnx",
    },
];

fn find_embedding_model(id: &str) -> Option<&'static SpeakerEmbeddingModelDef> {
    SPEAKER_EMBEDDING_MODELS.iter().find(|d| d.id == id)
}

// ---------------------------------------------------------------------------
// Whisper base (offline, multilingual — 99 languages)
// ---------------------------------------------------------------------------

const WHISPER_BASE_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-base/resolve/main/base-encoder.int8.onnx",
        dest_name: "whisper_base_encoder.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-base/resolve/main/base-decoder.int8.onnx",
        dest_name: "whisper_base_decoder.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-base/resolve/main/base-tokens.txt",
        dest_name: "whisper_base_tokens.txt",
    },
];

/// Whisper Turbo — largest/fastest Whisper variant, int8 (~1 GB total).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo
const WHISPER_TURBO_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/main/turbo-encoder.int8.onnx",
        dest_name: "whisper_turbo_encoder.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/main/turbo-decoder.int8.onnx",
        dest_name: "whisper_turbo_decoder.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-whisper-turbo/resolve/main/turbo-tokens.txt",
        dest_name: "whisper_turbo_tokens.txt",
    },
];

/// Zipformer CTC Small Chinese — offline, int8, ~63 MB.
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ctc-small-zh-int8-2025-07-16
const ZIPFORMER_CTC_ZH_SMALL_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ctc-small-zh-int8-2025-07-16/resolve/main/model.int8.onnx",
        dest_name: "zipformer_ctc_zh_small.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ctc-small-zh-int8-2025-07-16/resolve/main/tokens.txt",
        dest_name: "zipformer_ctc_zh_small_tokens.txt",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ctc-small-zh-int8-2025-07-16/resolve/main/bbpe.model",
        dest_name: "zipformer_ctc_zh_small_bbpe.model",
    },
];

/// Paraformer Chinese-small — offline bilingual zh+en, ~82 MB total.
/// Much smaller alternative to FunASR Nano (~1 GB).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09
const PARAFORMER_ZH_SMALL_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/model.int8.onnx",
        dest_name: "paraformer_zh_small.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-paraformer-zh-small-2024-03-09/resolve/main/tokens.txt",
        dest_name: "paraformer_zh_small_tokens.txt",
    },
];

/// Moonshine Tiny English — offline ASR, int8 quantised (~125 MB total).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8
const MOONSHINE_TINY_EN_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/preprocess.onnx",
        dest_name: "moonshine_tiny_en_preprocess.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/encode.int8.onnx",
        dest_name: "moonshine_tiny_en_encode.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/uncached_decode.int8.onnx",
        dest_name: "moonshine_tiny_en_uncached_decode.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/cached_decode.int8.onnx",
        dest_name: "moonshine_tiny_en_cached_decode.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-tiny-en-int8/resolve/main/tokens.txt",
        dest_name: "moonshine_tiny_en_tokens.txt",
    },
];

/// SenseVoice multilingual (zh/en/ja/ko/yue) — offline int8 (~239 MB).
/// Source: https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17
const SENSE_VOICE_MULTI_FILES: &[ModelFile] = &[
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx",
        dest_name: "sense_voice_multi.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt",
        dest_name: "sense_voice_multi_tokens.txt",
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
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/encoder_adaptor.int8.onnx",
        dest_name: "funasr_nano_encoder_adaptor.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/llm.int8.onnx",
        dest_name: "funasr_nano_llm.int8.onnx",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/Qwen3-0.6B/tokenizer.json",
        dest_name: "funasr_nano_tokenizer/tokenizer.json",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/Qwen3-0.6B/merges.txt",
        dest_name: "funasr_nano_tokenizer/merges.txt",
    },
    ModelFile {
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-funasr-nano-int8-2025-12-30/resolve/main/Qwen3-0.6B/vocab.json",
        dest_name: "funasr_nano_tokenizer/vocab.json",
    },
];

/// Return the download file list for a target. Streaming transducers are
/// generated from the table; offline models use static arrays.
fn files_for_target(target: &DownloadTarget) -> Result<Vec<ModelFile>> {
    match target {
        DownloadTarget::Shared => Ok(SHARED_FILES.to_vec()),
        DownloadTarget::Model(id) => {
            // Check the streaming transducer table first.
            if let Some(def) = find_transducer(id) {
                return Ok(transducer_model_files(def));
            }
            // Offline models with static file arrays.
            let slice: &[ModelFile] = match id.as_str() {
                "whisper_base" => WHISPER_BASE_FILES,
                "whisper_turbo" => WHISPER_TURBO_FILES,
                "zipformer_ctc_zh_small" => ZIPFORMER_CTC_ZH_SMALL_FILES,
                "paraformer_zh_small" => PARAFORMER_ZH_SMALL_FILES,
                "moonshine_tiny_en" => MOONSHINE_TINY_EN_FILES,
                "sense_voice_multi" => SENSE_VOICE_MULTI_FILES,
                "funasr_nano" => FUNASR_NANO_FILES,
                other => return Err(anyhow!("Unknown model pack: {}", other)),
            };
            Ok(slice.to_vec())
        }
        DownloadTarget::Embedding(id) => {
            let def = find_embedding_model(id)
                .ok_or_else(|| anyhow!("Unknown speaker embedding model: {}", id))?;
            Ok(vec![ModelFile {
                url: def.url,
                dest_name: def.dest_name,
            }])
        }
    }
}

/// Check whether all files for a given model id are present on disk.
fn model_downloaded(model_dir: &PathBuf, files: &[ModelFile]) -> bool {
    files.iter().all(|f| {
        let p = model_dir.join(f.dest_name);
        p.exists() && std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false)
    })
}

fn embedding_downloaded(model_dir: &PathBuf, def: &SpeakerEmbeddingModelDef) -> bool {
    let p = model_dir.join(def.dest_name);
    p.exists() && std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false)
}

/// Check whether a streaming transducer's files are present on disk.
fn transducer_downloaded(model_dir: &PathBuf, prefix: &str) -> bool {
    let enc = model_dir.join(format!("{prefix}_encoder.int8.onnx"));
    let dec = model_dir.join(format!("{prefix}_decoder.int8.onnx"));
    let join = model_dir.join(format!("{prefix}_joiner.int8.onnx"));
    let tok = model_dir.join(format!("{prefix}_tokens.txt"));
    enc.exists() && dec.exists() && join.exists() && tok.exists()
}

// ---------------------------------------------------------------------------
// ModelManager
// ---------------------------------------------------------------------------

pub struct ModelManager {
    model_dir: PathBuf,
    status: Arc<RwLock<ModelStatus>>,
    download_handle: Arc<tokio::sync::Mutex<Option<JoinHandle<()>>>>,
}

impl ModelManager {
    /// Create a new ModelManager. Extracts the bundled Silero VAD into the
    /// model directory on first startup so the shared tier is always ready.
    pub fn new(model_dir: PathBuf) -> Self {
        // Ensure model_dir exists before writing the bundled VAD.
        if let Err(e) = std::fs::create_dir_all(&model_dir) {
            warn!(error = %e, path = %model_dir.display(), "Failed to create model_dir");
        }
        if let Err(e) = ensure_bundled_vad(&model_dir) {
            warn!(error = %e, "Failed to extract bundled Silero VAD");
        }

        let status = detect_existing_status(&model_dir);
        info!(?status, model_dir = %model_dir.display(), "ModelManager initialised");
        Self {
            model_dir,
            status: Arc::new(RwLock::new(status)),
            download_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Return the current status (cloned).
    pub async fn status(&self) -> ModelStatus {
        self.status.read().await.clone()
    }

    /// Return info about available ASR models and their download status.
    /// Streaming transducers are generated from the table; offline models
    /// are appended individually.
    pub fn available_asr_models(&self) -> Vec<AsrModelInfo> {
        let d = &self.model_dir;
        let mut models: Vec<AsrModelInfo> = Vec::new();

        // ── Streaming transducers (from table) ───────────────────────
        for def in STREAMING_TRANSDUCERS {
            models.push(AsrModelInfo {
                id: def.id.to_string(),
                name: def.name.to_string(),
                languages: def.languages.to_string(),
                size_mb: def.size_mb,
                ram_mb: def.ram_mb,
                recommended_cpu: def.cpu.to_string(),
                streaming: true,
                description: def.description.to_string(),
                downloaded: transducer_downloaded(d, def.prefix),
                runtime_supported: true,
            });
        }

        // ── Offline models (individual entries) ──────────────────────
        models.push(AsrModelInfo {
            id: "whisper_base".to_string(),
            name: "Whisper Base (Multilingual)".to_string(),
            languages: "99 languages (auto-detect)".to_string(),
            size_mb: 161,
            ram_mb: 500,
            recommended_cpu: "Any modern CPU".to_string(),
            streaming: false,
            description: "OpenAI Whisper base, int8. Offline multilingual ASR \
                         with auto language detection. Processes utterances via VAD."
                .to_string(),
            downloaded: model_downloaded(d, WHISPER_BASE_FILES),
            runtime_supported: true,
        });
        models.push(AsrModelInfo {
            id: "whisper_turbo".to_string(),
            name: "Whisper Turbo (Multilingual)".to_string(),
            languages: "99 languages (auto-detect)".to_string(),
            size_mb: 1037,
            ram_mb: 2500,
            recommended_cpu: "CPU with AVX2, 8+ cores".to_string(),
            streaming: false,
            description: "OpenAI Whisper turbo, int8. Highest Whisper quality, \
                         fastest decoding. ~1 GB on disk. Processes utterances via VAD."
                .to_string(),
            downloaded: model_downloaded(d, WHISPER_TURBO_FILES),
            runtime_supported: true,
        });
        models.push(AsrModelInfo {
            id: "zipformer_ctc_zh_small".to_string(),
            name: "Zipformer CTC Small (Chinese)".to_string(),
            languages: "Chinese".to_string(),
            size_mb: 63,
            ram_mb: 300,
            recommended_cpu: "Any modern CPU".to_string(),
            streaming: false,
            description: "Offline Chinese ASR using Zipformer CTC. Small int8 model, \
                         fast inference. Processes utterances via VAD."
                .to_string(),
            downloaded: model_downloaded(d, ZIPFORMER_CTC_ZH_SMALL_FILES),
            runtime_supported: true,
        });
        models.push(AsrModelInfo {
            id: "paraformer_zh_small".to_string(),
            name: "Paraformer Small (Chinese + English)".to_string(),
            languages: "Chinese, English".to_string(),
            size_mb: 82,
            ram_mb: 400,
            recommended_cpu: "Any modern CPU".to_string(),
            streaming: false,
            description: "Offline bilingual Paraformer. Much smaller than FunASR \
                         Nano (~82 MB vs ~1 GB). Processes utterances via VAD."
                .to_string(),
            downloaded: model_downloaded(d, PARAFORMER_ZH_SMALL_FILES),
            runtime_supported: true,
        });
        models.push(AsrModelInfo {
            id: "moonshine_tiny_en".to_string(),
            name: "Moonshine Tiny (English)".to_string(),
            languages: "English".to_string(),
            size_mb: 125,
            ram_mb: 500,
            recommended_cpu: "CPU with AVX2".to_string(),
            streaming: false,
            description: "Offline English ASR from Useful Sensors. ~27M params, \
                         int8. Processes utterances via VAD."
                .to_string(),
            downloaded: model_downloaded(d, MOONSHINE_TINY_EN_FILES),
            runtime_supported: true,
        });
        models.push(AsrModelInfo {
            id: "sense_voice_multi".to_string(),
            name: "SenseVoice Multilingual".to_string(),
            languages: "Chinese, English, Japanese, Korean, Cantonese".to_string(),
            size_mb: 239,
            ram_mb: 800,
            recommended_cpu: "CPU with AVX2, 4+ cores".to_string(),
            streaming: false,
            description: "Offline multilingual ASR from FunAudioLLM. ~234M params, \
                         int8. Auto language detection. Processes utterances via VAD."
                .to_string(),
            downloaded: model_downloaded(d, SENSE_VOICE_MULTI_FILES),
            runtime_supported: true,
        });
        models.push(AsrModelInfo {
            id: "funasr_nano".to_string(),
            name: "FunASR Nano (Qwen3 0.6B)".to_string(),
            languages: "Chinese, English".to_string(),
            size_mb: 1010,
            ram_mb: 2000,
            recommended_cpu: "CPU with AVX2, 8+ cores".to_string(),
            streaming: false,
            description: "LLM-powered ASR with Qwen3-0.6B decoder. Highest quality \
                         but ~1 GB and slow on CPU. Not streaming."
                .to_string(),
            downloaded: model_downloaded(d, FUNASR_NANO_FILES),
            runtime_supported: true,
        });

        models
    }

    /// Return the speaker-embedding catalog with on-disk status.
    pub fn available_embedding_models(&self) -> Vec<SpeakerEmbeddingModelInfo> {
        SPEAKER_EMBEDDING_MODELS
            .iter()
            .map(|d| SpeakerEmbeddingModelInfo {
                id: d.id.to_string(),
                name: d.name.to_string(),
                description: d.description.to_string(),
                languages: d.languages.to_string(),
                size_mb: d.size_mb,
                embedding_dim: d.dim,
                downloaded: embedding_downloaded(&self.model_dir, d),
            })
            .collect()
    }

    /// Return paths to model files if the shared tier is present. Callers must
    /// still verify that at least one language pack is downloaded before
    /// attempting to start ASR.
    ///
    /// `speaker_embedding_id` selects which catalog entry resolves
    /// `ModelPaths.speaker_embedding`. Pass the id from
    /// `AudioSettings.speaker_embedding_model`. If no id is given or the chosen
    /// file isn't on disk, `build_paths` falls through to any known embedding
    /// filename found in `model_dir`.
    pub async fn model_paths(&self, speaker_embedding_id: Option<&str>) -> Option<ModelPaths> {
        let status = self.status.read().await;
        match &*status {
            ModelStatus::Ready => Some(build_paths(&self.model_dir, speaker_embedding_id)),
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
        let files: Vec<ModelFile> = if id == "shared" {
            SHARED_FILES.to_vec()
        } else if find_embedding_model(id).is_some() {
            files_for_target(&DownloadTarget::Embedding(id.to_string()))?
        } else {
            files_for_target(&DownloadTarget::Model(id.to_string()))?
        };

        // Guard: don't delete while a download is in flight — files could
        // be half-written and we'd race with the downloader.
        {
            let s = self.status.read().await;
            if matches!(&*s, ModelStatus::Downloading { .. }) {
                return Err(anyhow!("Cannot delete while a download is in progress"));
            }
        }

        let mut count: u32 = 0;
        let mut subdirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

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

        // Streaming transducers: no VAD needed.
        if let Some(def) = find_transducer(asr_model) {
            let files = transducer_model_files(def);
            return Ok(files.iter().map(|f| dir.join(f.dest_name)).collect());
        }

        // Offline models: need VAD prepended.
        let files = files_for_target(&DownloadTarget::Model(asr_model.to_string()))?;
        let mut paths: Vec<PathBuf> = vec![dir.join("silero_vad.onnx")];
        paths.extend(files.iter().map(|f| dir.join(f.dest_name)));
        Ok(paths)
    }

    /// Start downloading the given target in a background task. Progress is
    /// sent via the watch channel. The task updates the shared status on
    /// completion or error.
    pub async fn start_download(
        &self,
        target: DownloadTarget,
        source: DownloadSource,
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

        let handle = tokio::spawn(async move {
            let result =
                download_target(&model_dir, &target_clone, source, &status_arc, &progress_tx).await;

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

        *self.download_handle.lock().await = Some(handle);

        Ok(())
    }

    /// Cancel a running model download. Aborts the background task and resets
    /// status based on what is already on disk.
    pub async fn cancel_download(&self) {
        if let Some(handle) = self.download_handle.lock().await.take() {
            handle.abort();
        }
        let mut s = self.status.write().await;
        if matches!(&*s, ModelStatus::Downloading { .. }) {
            *s = detect_existing_status(&self.model_dir);
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the bundled Silero VAD into `model_dir/silero_vad.onnx` if the
/// file is missing or has zero size. Overwrites nothing — a user-deleted VAD
/// can be restored simply by relaunching the app.
fn ensure_bundled_vad(model_dir: &PathBuf) -> Result<()> {
    let dest = model_dir.join("silero_vad.onnx");
    let needs_write = match std::fs::metadata(&dest) {
        Ok(meta) => meta.len() == 0,
        Err(_) => true,
    };
    if !needs_write {
        return Ok(());
    }
    std::fs::write(&dest, SILERO_VAD_BYTES)
        .with_context(|| format!("writing bundled Silero VAD to {}", dest.display()))?;
    info!(bytes = SILERO_VAD_BYTES.len(), path = %dest.display(), "Extracted bundled Silero VAD");
    Ok(())
}

/// Detect whether the shared files are already downloaded. With the VAD now
/// bundled via `include_bytes!`, this should always be Ready after
/// `ModelManager::new` runs. Kept for robustness in case extraction failed.
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
        Some(TransducerFiles {
            encoder: enc,
            decoder: dec,
            joiner: join,
            tokens: tok,
        })
    } else {
        None
    }
}

fn build_paths(model_dir: &PathBuf, speaker_embedding_id: Option<&str>) -> ModelPaths {
    let opt = |name: &str| {
        let path = model_dir.join(name);
        if path.exists() {
            Some(path)
        } else {
            None
        }
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

    let zipformer_ctc_zh_small = {
        let model = model_dir.join("zipformer_ctc_zh_small.int8.onnx");
        let tokens = model_dir.join("zipformer_ctc_zh_small_tokens.txt");
        let bpe = model_dir.join("zipformer_ctc_zh_small_bbpe.model");
        if model.exists() && tokens.exists() {
            Some(ZipformerCtcFiles {
                model,
                tokens,
                bpe_vocab: if bpe.exists() { Some(bpe) } else { None },
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

    // Build all streaming transducers from the table.
    let mut transducers = std::collections::HashMap::new();
    for def in STREAMING_TRANSDUCERS {
        if let Some(files) = build_transducer(model_dir, def.prefix) {
            transducers.insert(def.id.to_string(), files);
        }
    }

    let whisper_base = {
        let encoder = model_dir.join("whisper_base_encoder.int8.onnx");
        let decoder = model_dir.join("whisper_base_decoder.int8.onnx");
        let tokens = model_dir.join("whisper_base_tokens.txt");
        if encoder.exists() && decoder.exists() && tokens.exists() {
            Some(WhisperFiles {
                encoder,
                decoder,
                tokens,
            })
        } else {
            None
        }
    };

    let whisper_turbo = {
        let encoder = model_dir.join("whisper_turbo_encoder.int8.onnx");
        let decoder = model_dir.join("whisper_turbo_decoder.int8.onnx");
        let tokens = model_dir.join("whisper_turbo_tokens.txt");
        if encoder.exists() && decoder.exists() && tokens.exists() {
            Some(WhisperFiles {
                encoder,
                decoder,
                tokens,
            })
        } else {
            None
        }
    };

    ModelPaths {
        silero_vad: model_dir.join("silero_vad.onnx"),
        transducers,
        sense_voice,
        moonshine_en,
        zipformer_ctc_zh_small,
        paraformer_zh_small,
        funasr_nano,
        whisper_base,
        whisper_turbo,
        pyannote_segmentation: opt("pyannote-seg3.onnx"),
        // Prefer the caller's selection; fall back to any known embedding
        // filename on disk so users from the pre-catalog era (or who placed
        // a file manually) aren't stranded.
        speaker_embedding: speaker_embedding_id
            .and_then(find_embedding_model)
            .and_then(|def| opt(def.dest_name))
            .or_else(|| opt("speaker_campplus_zh_en.onnx"))
            .or_else(|| opt("speaker_campplus_zh.onnx"))
            .or_else(|| opt("speaker_campplus_en.onnx"))
            .or_else(|| opt("speaker_eres2netv2.onnx"))
            .or_else(|| opt("speaker_eres2net_base.onnx"))
            .or_else(|| opt("speaker_titanet_small.onnx"))
            .or_else(|| opt("speaker_eres2net.onnx")),
    }
}

/// Rewrite a HuggingFace URL for the chosen download source.
///
/// All ASR model URLs in the catalog use `huggingface.co`. For HfMirror we
/// swap the domain to `hf-mirror.com` (a full public mirror of HuggingFace).
/// ModelScope is not supported for ASR models — most sherpa-onnx repos are
/// not mirrored there — so it falls back to HuggingFace. GitHub URLs
/// (e.g. Silero VAD) are left untouched.
fn rewrite_url(url: &str, source: DownloadSource) -> String {
    match source {
        DownloadSource::HuggingFace | DownloadSource::ModelScope => url.to_string(),
        DownloadSource::HfMirror => {
            url.replace("https://huggingface.co/", "https://hf-mirror.com/")
        }
    }
}

/// Download all files for a target, updating status along the way.
async fn download_target(
    model_dir: &PathBuf,
    target: &DownloadTarget,
    source: DownloadSource,
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

        download_file_with_retry(&client, file, model_dir, source).await?;

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
    source: DownloadSource,
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

    let url = rewrite_url(file.url, source);

    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=3u8 {
        match do_download(client, &url, file.dest_name, &dest).await {
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
    url: &str,
    dest_name: &str,
    dest: &PathBuf,
) -> Result<()> {
    info!(url, dest = %dest.display(), "Downloading model file");

    // Ensure parent directory exists (dest_name may include subdirs,
    // e.g. "funasr_nano_tokenizer/tokenizer.json").
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating parent dir {}", parent.display()))?;
    }

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?
        .error_for_status()
        .with_context(|| format!("HTTP error for {}", url))?;

    // All current model files are plain downloads; no archive extraction is needed.
    let _ = dest_name;
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("creating {}", dest.display()))?;
    let mut stream = response.bytes_stream();
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("reading response body from {}", url))?;
        file.write_all(&chunk)
            .await
            .with_context(|| format!("writing {}", dest.display()))?;
    }
    file.flush()
        .await
        .with_context(|| format!("flushing {}", dest.display()))?;

    // Verify non-zero size
    let meta = std::fs::metadata(dest)?;
    if meta.len() == 0 {
        return Err(anyhow!("Downloaded file {} has zero size", dest.display()));
    }

    Ok(())
}

