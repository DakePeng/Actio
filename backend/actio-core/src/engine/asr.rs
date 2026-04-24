use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::engine::model_manager::{
    FunAsrNanoFiles, MoonshineFiles, ParaformerFiles, SenseVoiceFiles, TransducerFiles,
    WhisperFiles, ZipformerCtcFiles,
};
use crate::engine::vad::SpeechSegment;

/// A transcript result emitted by ASR engines.
#[derive(Debug, Clone)]
pub struct TranscriptResult {
    pub text: String,
    pub is_final: bool,
    pub start_sample: usize,
    pub end_sample: usize,
    pub segment_id: Option<Uuid>,
}

/// Start a streaming ASR task fed directly with raw audio chunks (16kHz mono f32).
/// Bypasses VAD for minimal latency — the OnlineRecognizer's built-in endpoint
/// detection handles utterance boundaries.
///
/// Emits partial results as audio is decoded and final results on endpoint detection.
///
/// OnlineRecognizer contains raw pointers and is !Send. The entire recognizer
/// lifecycle runs inside a single `spawn_blocking` call via a crossbeam channel
/// bridge, matching the same pattern used in engine/vad.rs.
pub fn start_streaming_asr(
    model_files: &TransducerFiles,
    mut audio_rx: mpsc::Receiver<Vec<f32>>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    let encoder = model_files.encoder.to_string_lossy().to_string();
    let decoder = model_files.decoder.to_string_lossy().to_string();
    let joiner = model_files.joiner.to_string_lossy().to_string();
    let tokens = model_files.tokens.to_string_lossy().to_string();

    // Sync bridge: tokio audio_rx → blocking ASR thread. Unbounded so audio
    // captured during model load (wake-from-hibernation) queues up instead of
    // being dropped. The recognizer processes backlog faster than real-time.
    let (audio_tx, audio_cb_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    // Sync bridge: blocking ASR thread → tokio transcript consumer
    let (result_cb_tx, result_rx) = crossbeam_channel::bounded::<TranscriptResult>(64);

    // Task 1: drain tokio mpsc into crossbeam for the blocking thread
    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            if audio_tx.send(chunk).is_err() {
                break;
            }
        }
    });

    // Task 2: bridge crossbeam results back to tokio mpsc
    let (result_tokio_tx, result_tokio_rx) = mpsc::channel::<TranscriptResult>(64);
    tokio::spawn(async move {
        loop {
            match result_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(result) => {
                    if result_tokio_tx.send(result).await.is_err() {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // Task 3: blocking thread owns OnlineRecognizer for its entire lifetime
    tokio::task::spawn_blocking(move || {
        let mut config = sherpa_onnx::OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = Some(encoder);
        config.model_config.transducer.decoder = Some(decoder);
        config.model_config.transducer.joiner = Some(joiner);
        config.model_config.tokens = Some(tokens);
        config.model_config.num_threads = 2;
        config.model_config.provider = Some("cpu".to_string());
        config.enable_endpoint = true;
        config.decoding_method = Some("greedy_search".to_string());

        let recognizer = match sherpa_onnx::OnlineRecognizer::create(&config) {
            Some(r) => r,
            None => {
                tracing::error!("Failed to create OnlineRecognizer — check model paths");
                return;
            }
        };

        info!("Streaming ASR (Zipformer) initialized");

        let stream = recognizer.create_stream();
        let mut total_samples: usize = 0;
        let mut last_partial = String::new();

        loop {
            match audio_cb_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(chunk) => {
                    let chunk_len = chunk.len();
                    stream.accept_waveform(16000, &chunk);
                    total_samples += chunk_len;

                    while recognizer.is_ready(&stream) {
                        recognizer.decode(&stream);
                    }

                    // Emit partial if text changed
                    if let Some(result) = recognizer.get_result(&stream) {
                        let text = result.text.trim().to_string();
                        if !text.is_empty() && text != last_partial {
                            last_partial = text.clone();
                            let _ = result_cb_tx.try_send(TranscriptResult {
                                text,
                                is_final: false,
                                start_sample: 0,
                                end_sample: total_samples,
                                segment_id: None,
                            });
                        }
                    }

                    // Endpoint detected → emit final, reset
                    if recognizer.is_endpoint(&stream) {
                        if let Some(result) = recognizer.get_result(&stream) {
                            let text = result.text.trim().to_string();
                            if !text.is_empty() {
                                info!(text = %text, "ASR final");
                                let _ = result_cb_tx.send(TranscriptResult {
                                    text,
                                    is_final: true,
                                    start_sample: 0,
                                    end_sample: total_samples,
                                    segment_id: None,
                                });
                            }
                        }
                        recognizer.reset(&stream);
                        last_partial.clear();
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        info!("Streaming ASR thread ended");
    });

    Ok(result_tokio_rx)
}

/// Start a non-streaming ASR task using SenseVoice. Processes complete speech
/// segments and emits one final transcript per segment.
pub fn start_sense_voice_asr(
    files: &SenseVoiceFiles,
    speech_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    let sense_voice_model = files.model.to_string_lossy().to_string();
    let tokens = files.tokens.to_string_lossy().to_string();

    let config_builder = move || {
        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
        config.model_config.sense_voice = sherpa_onnx::OfflineSenseVoiceModelConfig {
            model: Some(sense_voice_model),
            language: Some("auto".to_string()),
            use_itn: true,
        };
        config.model_config.tokens = Some(tokens);
        config.model_config.num_threads = 1;
        config.model_config.provider = Some("cpu".to_string());
        config
    };

    spawn_offline_asr_loop("SenseVoice", Box::new(config_builder), speech_rx)
}

/// Start a non-streaming ASR task using Moonshine v1 (preprocessor + encoder +
/// cached/uncached decoders). English-only.
pub fn start_moonshine_asr(
    files: &MoonshineFiles,
    speech_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    let preprocessor = files.preprocessor.to_string_lossy().to_string();
    let encoder = files.encoder.to_string_lossy().to_string();
    let uncached_decoder = files.uncached_decoder.to_string_lossy().to_string();
    let cached_decoder = files.cached_decoder.to_string_lossy().to_string();
    let tokens = files.tokens.to_string_lossy().to_string();

    let config_builder = move || {
        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
        config.model_config.moonshine = sherpa_onnx::OfflineMoonshineModelConfig {
            preprocessor: Some(preprocessor),
            encoder: Some(encoder),
            uncached_decoder: Some(uncached_decoder),
            cached_decoder: Some(cached_decoder),
            ..Default::default()
        };
        config.model_config.tokens = Some(tokens);
        config.model_config.num_threads = 2;
        config.model_config.provider = Some("cpu".to_string());
        config
    };

    spawn_offline_asr_loop("Moonshine", Box::new(config_builder), speech_rx)
}

/// Start a non-streaming ASR task using an offline Paraformer model
/// (single `model.int8.onnx` + `tokens.txt`). Used for the small bilingual
/// Chinese + English Paraformer.
pub fn start_paraformer_offline_asr(
    files: &ParaformerFiles,
    speech_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    let model = files.model.to_string_lossy().to_string();
    let tokens = files.tokens.to_string_lossy().to_string();

    let config_builder = move || {
        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
        config.model_config.paraformer =
            sherpa_onnx::OfflineParaformerModelConfig { model: Some(model) };
        config.model_config.tokens = Some(tokens);
        config.model_config.num_threads = 2;
        config.model_config.provider = Some("cpu".to_string());
        config
    };

    spawn_offline_asr_loop("Paraformer", Box::new(config_builder), speech_rx)
}

/// Start a non-streaming ASR task using an offline Zipformer CTC model.
pub fn start_zipformer_ctc_asr(
    files: &ZipformerCtcFiles,
    speech_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    let model = files.model.to_string_lossy().to_string();
    let tokens = files.tokens.to_string_lossy().to_string();
    let bpe_vocab = files
        .bpe_vocab
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());

    let config_builder = move || {
        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
        config.model_config.zipformer_ctc =
            sherpa_onnx::OfflineZipformerCtcModelConfig { model: Some(model) };
        config.model_config.tokens = Some(tokens);
        if let Some(bpe) = bpe_vocab {
            config.model_config.bpe_vocab = Some(bpe);
        }
        config.model_config.num_threads = 2;
        config.model_config.provider = Some("cpu".to_string());
        config
    };

    spawn_offline_asr_loop("ZipformerCTC", Box::new(config_builder), speech_rx)
}

/// Start a non-streaming ASR task using FunASR Nano (Qwen3-0.6B cascaded
/// LLM-ASR). Expensive to load and run — expect multi-second latency per
/// segment on CPU. Supports Chinese and English.
pub fn start_funasr_nano_asr(
    files: &FunAsrNanoFiles,
    speech_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    let encoder_adaptor = files.encoder_adaptor.to_string_lossy().to_string();
    let llm = files.llm.to_string_lossy().to_string();
    let embedding = files.embedding.to_string_lossy().to_string();
    let tokenizer_dir = files.tokenizer_dir.to_string_lossy().to_string();

    let config_builder = move || {
        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
        config.model_config.funasr_nano = sherpa_onnx::OfflineFunASRNanoModelConfig {
            encoder_adaptor: Some(encoder_adaptor),
            llm: Some(llm),
            embedding: Some(embedding),
            tokenizer: Some(tokenizer_dir),
            language: Some("auto".to_string()),
            itn: 1,
            ..Default::default()
        };
        config.model_config.num_threads = 4;
        config.model_config.provider = Some("cpu".to_string());
        config
    };

    spawn_offline_asr_loop("FunASR Nano", Box::new(config_builder), speech_rx)
}

/// Start a non-streaming ASR task using OpenAI Whisper (encoder + decoder).
/// Supports ~99 languages with auto-detection.
pub fn start_whisper_asr(
    files: &WhisperFiles,
    speech_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    let encoder = files.encoder.to_string_lossy().to_string();
    let decoder = files.decoder.to_string_lossy().to_string();
    let tokens = files.tokens.to_string_lossy().to_string();

    let config_builder = move || {
        let mut config = sherpa_onnx::OfflineRecognizerConfig::default();
        config.model_config.whisper = sherpa_onnx::OfflineWhisperModelConfig {
            encoder: Some(encoder),
            decoder: Some(decoder),
            language: Some(String::new()), // empty = auto-detect
            task: Some("transcribe".to_string()),
            ..Default::default()
        };
        config.model_config.tokens = Some(tokens);
        config.model_config.num_threads = 2;
        config.model_config.provider = Some("cpu".to_string());
        config
    };

    spawn_offline_asr_loop("Whisper", Box::new(config_builder), speech_rx)
}

/// Internal: run an OfflineRecognizer in a blocking thread, fed by VAD
/// segments. The config is built inside the blocking thread so CString
/// backing storage stays on the same thread that owns the recognizer.
///
/// OfflineRecognizer contains raw pointers and is !Send. The entire
/// recognizer lifecycle runs inside a single `spawn_blocking` call, same
/// pattern as vad.rs.
fn spawn_offline_asr_loop(
    model_name: &'static str,
    build_config: Box<dyn FnOnce() -> sherpa_onnx::OfflineRecognizerConfig + Send>,
    mut speech_rx: mpsc::Receiver<SpeechSegment>,
) -> anyhow::Result<mpsc::Receiver<TranscriptResult>> {
    // Sync bridge: tokio speech_rx → blocking ASR thread
    let (seg_tx, seg_cb_rx) = crossbeam_channel::bounded::<SpeechSegment>(32);
    // Sync bridge: blocking ASR thread → tokio transcript consumer
    let (result_cb_tx, result_rx) = crossbeam_channel::bounded::<TranscriptResult>(64);

    // Task 1: drain tokio mpsc into crossbeam for the blocking thread
    tokio::spawn(async move {
        while let Some(seg) = speech_rx.recv().await {
            if seg_tx.send(seg).is_err() {
                break;
            }
        }
    });

    // Task 2: bridge crossbeam results back to tokio mpsc
    let (result_tokio_tx, result_tokio_rx) = mpsc::channel::<TranscriptResult>(64);
    tokio::spawn(async move {
        loop {
            match result_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(result) => {
                    if result_tokio_tx.send(result).await.is_err() {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    // Task 3: blocking thread owns OfflineRecognizer for its entire lifetime
    tokio::task::spawn_blocking(move || {
        let config = build_config();
        let recognizer = match sherpa_onnx::OfflineRecognizer::create(&config) {
            Some(r) => r,
            None => {
                warn!(
                    model = model_name,
                    "Failed to create OfflineRecognizer — check model files"
                );
                return;
            }
        };

        info!(model = model_name, "Offline ASR initialized");

        loop {
            match seg_cb_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(segment) => {
                    let segment_id = segment.segment_id;
                    let start_sample = segment.start_sample;
                    let end_sample = segment.end_sample;

                    let stream = recognizer.create_stream();
                    stream.accept_waveform(16000, &segment.audio);
                    recognizer.decode(&stream);

                    if let Some(result) = stream.get_result() {
                        if !result.text.trim().is_empty() {
                            info!(model = model_name, text = %result.text, "Offline ASR result");
                            let t = TranscriptResult {
                                text: result.text,
                                is_final: true,
                                start_sample,
                                end_sample,
                                segment_id: Some(segment_id),
                            };
                            if result_cb_tx.send(t).is_err() {
                                info!(
                                    model = model_name,
                                    "Offline ASR thread ended — result consumer dropped"
                                );
                                return;
                            }
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        info!(model = model_name, "Offline ASR thread ended");
    });

    Ok(result_tokio_rx)
}
