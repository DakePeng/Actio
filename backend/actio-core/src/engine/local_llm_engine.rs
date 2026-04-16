use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use crate::engine::llm_catalog::{available_local_llms, DownloadSource, LocalLlmInfo};
use crate::engine::llm_prompt::ChatMessage;

#[derive(Debug, thiserror::Error)]
pub enum LocalLlmError {
    #[error("model {0} is not in the catalog")]
    UnknownModel(String),
    #[error("model is not downloaded")]
    NotDownloaded,
    #[error("failed to load model: {0}")]
    LoadFailed(String),
    #[error("model file is corrupt or truncated: {0}")]
    CorruptModelFile(String),
    #[error("out of memory loading {model_id} (needed ~{needed_mb} MB)")]
    OutOfMemory { model_id: String, needed_mb: u32 },
    #[error("CPU does not support required instructions: {0}")]
    UnsupportedCpu(String),
    #[error("inference failed: {0}")]
    InferenceFailed(String),
    #[error("local LLM feature not compiled into this build")]
    FeatureDisabled,
}

#[derive(Debug, Clone)]
pub struct GenerationParams {
    pub max_tokens: usize,
    pub temperature: f32,
    pub json_mode: bool,
    /// If Some(n), inject `<think>\n` after the chat template to activate
    /// Qwen3-style chain-of-thought. The model's thinking tokens count
    /// against max_tokens, so we add this budget on top automatically.
    pub thinking_budget: Option<usize>,
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            max_tokens: 2000,
            temperature: 0.1,
            json_mode: false,
            thinking_budget: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePriority {
    Internal,
    External,
}

// ---------------------------------------------------------------------------
// Real implementation (cfg local-llm)
// ---------------------------------------------------------------------------

#[cfg(feature = "local-llm")]
use std::sync::OnceLock;

#[cfg(feature = "local-llm")]
use llama_cpp_2::llama_backend::LlamaBackend;

/// Process-global singleton. `LlamaBackend::init()` can only be called once.
#[cfg(feature = "local-llm")]
static LLAMA_BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

#[cfg(feature = "local-llm")]
fn get_or_init_backend() -> &'static LlamaBackend {
    LLAMA_BACKEND.get_or_init(|| {
        let mut backend = LlamaBackend::init().expect("failed to initialize llama.cpp backend");
        backend.void_logs();
        backend
    })
}

#[cfg(feature = "local-llm")]
pub struct LocalLlmEngine {
    loaded_id: String,
    metadata: LocalLlmInfo,
    model: Arc<llama_cpp_2::model::LlamaModel>,
    waiting_internal: std::sync::atomic::AtomicUsize,
}

#[cfg(feature = "local-llm")]
impl LocalLlmEngine {
    /// Path where a model's GGUF file is stored.
    pub fn gguf_path(model_dir: &Path, info: &LocalLlmInfo) -> PathBuf {
        model_dir.join("llms").join(&info.id).join(&info.gguf_filename)
    }

    /// Check if a model's GGUF file exists on disk.
    pub fn has_gguf(model_dir: &Path, info: &LocalLlmInfo) -> bool {
        Self::gguf_path(model_dir, info).exists()
    }

    pub async fn load_async(model_dir: &Path, info: &LocalLlmInfo) -> Result<Self, LocalLlmError> {
        let overall_start = std::time::Instant::now();
        let gguf_path = Self::gguf_path(model_dir, info);
        info!(
            model_id = %info.id,
            gguf = %gguf_path.display(),
            ram_mb = info.ram_mb,
            "load_async: starting"
        );

        if !gguf_path.exists() {
            return Err(LocalLlmError::NotDownloaded);
        }

        let info_clone = info.clone();
        let model = tokio::task::spawn_blocking(move || {
            let backend = get_or_init_backend();
            let params = llama_cpp_2::model::params::LlamaModelParams::default();
            let t = std::time::Instant::now();
            let m = llama_cpp_2::model::LlamaModel::load_from_file(backend, &gguf_path, &params)
                .map_err(|e| classify_load_error(&info_clone.id, info_clone.ram_mb, e))?;
            info!(
                model_id = %info_clone.id,
                elapsed_ms = t.elapsed().as_millis() as u64,
                "load_async: GGUF load complete"
            );
            Ok::<_, LocalLlmError>(m)
        })
        .await
        .map_err(|e| LocalLlmError::LoadFailed(format!("spawn_blocking panicked: {e}")))??;

        info!(
            model_id = %info.id,
            total_ms = overall_start.elapsed().as_millis() as u64,
            "load_async: engine ready"
        );
        Ok(Self {
            loaded_id: info.id.clone(),
            metadata: info.clone(),
            model: Arc::new(model),
            waiting_internal: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        params: GenerationParams,
        priority: EnginePriority,
    ) -> Result<String, LocalLlmError> {
        use std::sync::atomic::Ordering;
        info!(
            model = %self.loaded_id,
            msg_count = messages.len(),
            max_tokens = params.max_tokens,
            temperature = params.temperature,
            json_mode = params.json_mode,
            ?priority,
            "chat_completion: request received"
        );

        if priority == EnginePriority::Internal {
            self.waiting_internal.fetch_add(1, Ordering::SeqCst);
        }

        if priority == EnginePriority::External {
            let waiting = self.waiting_internal.load(Ordering::SeqCst);
            if waiting > 0 {
                info!(waiting_internal = waiting, "chat_completion: waiting for internal requests to drain");
            }
            for _ in 0..5 {
                if self.waiting_internal.load(Ordering::SeqCst) == 0 {
                    break;
                }
                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        if priority == EnginePriority::Internal {
            self.waiting_internal.fetch_sub(1, Ordering::SeqCst);
        }

        let model = Arc::clone(&self.model);
        let loaded_id = self.loaded_id.clone();

        info!(model = %loaded_id, "chat_completion: sending to model");
        let t = std::time::Instant::now();

        let content = tokio::task::spawn_blocking(move || {
            run_inference(&model, &messages, &params)
        })
        .await
        .map_err(|e| LocalLlmError::InferenceFailed(format!("spawn_blocking panicked: {e}")))??;

        info!(
            model = %self.loaded_id,
            elapsed_ms = t.elapsed().as_millis() as u64,
            response_len = content.len(),
            "chat_completion: done"
        );

        Ok(content)
    }

    pub fn loaded_id(&self) -> &str {
        &self.loaded_id
    }

    pub fn metadata(&self) -> &LocalLlmInfo {
        &self.metadata
    }
}

/// Run inference synchronously. Called inside `spawn_blocking`.
#[cfg(feature = "local-llm")]
fn run_inference(
    model: &llama_cpp_2::model::LlamaModel,
    messages: &[ChatMessage],
    params: &GenerationParams,
) -> Result<String, LocalLlmError> {
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::model::LlamaChatMessage;
    use llama_cpp_2::sampling::LlamaSampler;

    let backend = get_or_init_backend();

    // 1. Create context — use 4096 tokens runtime context (not the model's full 262K)
    let n_ctx = std::num::NonZeroU32::new(4096).unwrap();
    let ctx_params = LlamaContextParams::default().with_n_ctx(Some(n_ctx));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| LocalLlmError::InferenceFailed(format!("context creation failed: {e}")))?;

    // 2. Apply chat template from GGUF metadata
    let chat_messages: Vec<LlamaChatMessage> = messages
        .iter()
        .map(|m| {
            LlamaChatMessage::new(m.role.clone(), m.content.clone())
                .map_err(|e| LocalLlmError::InferenceFailed(format!("chat message error: {e}")))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let template = model
        .chat_template(None)
        .map_err(|e| LocalLlmError::InferenceFailed(format!("chat template error: {e}")))?;

    let mut prompt = model
        .apply_chat_template(&template, &chat_messages, true)
        .map_err(|e| LocalLlmError::InferenceFailed(format!("apply template error: {e}")))?;

    // Activate Qwen3 chain-of-thought by injecting <think> after the
    // assistant generation prompt. The model continues inside the think
    // block and closes it before emitting the final answer.
    if params.thinking_budget.is_some() {
        prompt.push_str("<think>\n");
    }

    // 3. Tokenize
    let tokens = model
        .str_to_token(&prompt, llama_cpp_2::model::AddBos::Always)
        .map_err(|e| LocalLlmError::InferenceFailed(format!("tokenization error: {e}")))?;

    if tokens.len() >= 4096 {
        return Err(LocalLlmError::InferenceFailed(format!(
            "prompt too long: {} tokens (max 4096)",
            tokens.len()
        )));
    }

    // 4. Create batch and fill with prompt tokens
    let mut batch = llama_cpp_2::llama_batch::LlamaBatch::get_one(&tokens)
        .map_err(|e| LocalLlmError::InferenceFailed(format!("batch creation error: {e}")))?;

    // 5. Decode prompt
    ctx.decode(&mut batch)
        .map_err(|e| LocalLlmError::InferenceFailed(format!("prompt decode error: {e}")))?;

    // 6. Set up sampler
    let seed = rand_seed();
    let mut sampler = if params.temperature <= 0.0 {
        LlamaSampler::chain_simple([LlamaSampler::greedy()])
    } else {
        LlamaSampler::chain_simple([
            LlamaSampler::temp(params.temperature),
            LlamaSampler::dist(seed),
        ])
    };

    // 7. Generate tokens
    // After prompt decode, logits are at the last position in the batch.
    // sampler.sample() takes a batch-local index: for the initial prompt
    // batch that's (tokens.len() - 1), for subsequent single-token batches
    // it's always 0.
    let mut output_tokens = Vec::new();
    let mut sample_idx = (tokens.len() - 1) as i32;

    let thinking_budget = params.thinking_budget.unwrap_or(0);
    let token_limit = params.max_tokens + thinking_budget;
    let mut in_thinking = params.thinking_budget.is_some(); // we injected <think>\n
    let mut thinking_tokens: usize = 0;

    // Live-decode buffer so we can detect </think> and cut thinking mid-stream
    let mut live_decoder = encoding_rs::UTF_8.new_decoder();
    let mut live_text = String::new();

    for _ in 0..token_limit {
        let token = sampler.sample(&ctx, sample_idx);
        sampler.accept(token);

        // Check for end of generation
        if model.is_eog_token(token) {
            break;
        }

        output_tokens.push(token);

        // Live-decode for thinking detection
        if in_thinking {
            if let Ok(piece) = model.token_to_piece(token, &mut live_decoder, false, None) {
                live_text.push_str(&piece);
            }
            thinking_tokens += 1;

            // Check if model naturally closed thinking
            if live_text.contains("</think>") {
                in_thinking = false;
            }
            // Budget exceeded — force-inject </think>\n and stop counting thinking tokens
            else if thinking_tokens >= thinking_budget {
                tracing::info!(thinking_tokens, "Thinking budget hit, force-closing think block");
                let close_tag = "</think>\n";
                let close_tokens = model
                    .str_to_token(close_tag, llama_cpp_2::model::AddBos::Never)
                    .unwrap_or_default();
                for &ct in &close_tokens {
                    output_tokens.push(ct);
                    let close_arr = [ct];
                    let mut close_batch = llama_cpp_2::llama_batch::LlamaBatch::get_one(&close_arr)
                        .map_err(|e| LocalLlmError::InferenceFailed(format!("batch error: {e}")))?;
                    ctx.decode(&mut close_batch)
                        .map_err(|e| LocalLlmError::InferenceFailed(format!("decode error: {e}")))?;
                }
                in_thinking = false;
                sample_idx = 0;
                continue;
            }
        }

        // Decode next token — single-token batch, logits at index 0
        let next_tokens = [token];
        let mut next_batch = llama_cpp_2::llama_batch::LlamaBatch::get_one(&next_tokens)
            .map_err(|e| LocalLlmError::InferenceFailed(format!("batch error: {e}")))?;
        ctx.decode(&mut next_batch)
            .map_err(|e| LocalLlmError::InferenceFailed(format!("decode error: {e}")))?;
        sample_idx = 0;
    }

    // 8. Convert tokens to string
    let mut content = String::new();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    for &token in &output_tokens {
        let piece = model
            .token_to_piece(token, &mut decoder, false, None)
            .map_err(|e| LocalLlmError::InferenceFailed(format!("detokenization error: {e}")))?;
        content.push_str(&piece);
    }

    Ok(content)
}

#[cfg(feature = "local-llm")]
fn rand_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(42)
}

#[cfg(feature = "local-llm")]
fn classify_load_error(
    model_id: &str,
    ram_mb: u32,
    e: impl std::fmt::Display,
) -> LocalLlmError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("out of memory") || lower.contains("oom") || lower.contains("allocation") {
        LocalLlmError::OutOfMemory {
            model_id: model_id.into(),
            needed_mb: ram_mb,
        }
    } else if lower.contains("avx") || lower.contains("instruction") || lower.contains("cpu") {
        LocalLlmError::UnsupportedCpu(msg)
    } else if lower.contains("magic") || lower.contains("invalid gguf") || lower.contains("truncat") {
        LocalLlmError::CorruptModelFile(msg)
    } else {
        LocalLlmError::LoadFailed(msg)
    }
}

// ---------------------------------------------------------------------------
// Stub implementation (cfg NOT local-llm)
// ---------------------------------------------------------------------------

#[cfg(not(feature = "local-llm"))]
pub struct LocalLlmEngine {
    _never: std::marker::PhantomData<()>,
}

#[cfg(not(feature = "local-llm"))]
impl LocalLlmEngine {
    pub fn gguf_path(_model_dir: &Path, _info: &LocalLlmInfo) -> PathBuf {
        PathBuf::new()
    }

    pub fn has_gguf(_model_dir: &Path, _info: &LocalLlmInfo) -> bool {
        false
    }

    pub async fn load_async(_model_dir: &Path, _info: &LocalLlmInfo) -> Result<Self, LocalLlmError> {
        Err(LocalLlmError::FeatureDisabled)
    }

    pub async fn chat_completion(
        &self,
        _messages: Vec<ChatMessage>,
        _params: GenerationParams,
        _priority: EnginePriority,
    ) -> Result<String, LocalLlmError> {
        Err(LocalLlmError::FeatureDisabled)
    }

    pub fn loaded_id(&self) -> &str {
        ""
    }

    pub fn metadata(&self) -> &LocalLlmInfo {
        unreachable!("metadata() called on stubbed LocalLlmEngine")
    }
}

// ---------------------------------------------------------------------------
// EngineSlot — lazy-sticky lifecycle holder with loading status
// ---------------------------------------------------------------------------

use serde::Serialize;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LoadStatus {
    Idle,
    Downloading { llm_id: String, progress: f32 },
    /// Deprecated: kept for API compat. GGUF models are pre-quantized.
    Quantizing { llm_id: String },
    Loading { llm_id: String },
    Loaded { llm_id: String },
    Error { llm_id: String, message: String },
}

pub struct EngineSlot {
    model_dir: PathBuf,
    current: Mutex<Option<Arc<LocalLlmEngine>>>,
    load_status: Arc<RwLock<LoadStatus>>,
    load_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Prevents overlapping model loads from consuming double RAM.
    load_guard: tokio::sync::Notify,
}

impl EngineSlot {
    pub fn new(model_dir: PathBuf) -> Self {
        Self {
            model_dir,
            current: Mutex::new(None),
            load_status: Arc::new(RwLock::new(LoadStatus::Idle)),
            load_handle: Mutex::new(None),
            load_guard: tokio::sync::Notify::new(),
        }
    }

    pub async fn load_status(&self) -> LoadStatus {
        self.load_status.read().await.clone()
    }

    pub async fn get_or_load(&self, desired_id: &str) -> Result<Arc<LocalLlmEngine>, LocalLlmError> {
        info!(llm_id = %desired_id, "get_or_load: requested");
        let mut guard = self.current.lock().await;
        if let Some(engine) = guard.as_ref() {
            if engine.loaded_id() == desired_id {
                info!(llm_id = %desired_id, "get_or_load: already loaded, returning cached engine");
                return Ok(Arc::clone(engine));
            }
            info!(loaded = %engine.loaded_id(), requested = %desired_id, "get_or_load: different model loaded, unloading");
            *guard = None;
        }
        let info = available_local_llms()
            .into_iter()
            .find(|m| m.id == desired_id)
            .ok_or_else(|| LocalLlmError::UnknownModel(desired_id.into()))?;

        let has_gguf = LocalLlmEngine::has_gguf(&self.model_dir, &info);

        info!(
            llm_id = %desired_id,
            has_gguf,
            "get_or_load: determined load strategy"
        );

        if !has_gguf {
            return Err(LocalLlmError::NotDownloaded);
        }

        // Set loading status
        {
            info!(llm_id = %desired_id, "get_or_load: setting loading status");
            let mut s = self.load_status.write().await;
            *s = LoadStatus::Loading { llm_id: desired_id.to_string() };
        }

        let result = LocalLlmEngine::load_async(&self.model_dir, &info).await;

        match result {
            Ok(engine) => {
                info!(llm_id = %desired_id, "get_or_load: engine built successfully, storing");
                let engine = Arc::new(engine);
                *guard = Some(Arc::clone(&engine));
                {
                    let mut s = self.load_status.write().await;
                    *s = LoadStatus::Loaded { llm_id: desired_id.to_string() };
                }
                Ok(engine)
            }
            Err(e) => {
                tracing::error!(llm_id = %desired_id, error = %e, "get_or_load: engine build failed");
                {
                    let mut s = self.load_status.write().await;
                    *s = LoadStatus::Error {
                        llm_id: desired_id.to_string(),
                        message: e.to_string(),
                    };
                }
                Err(e)
            }
        }
    }

    /// Start loading a model in the background. Downloads the GGUF if needed.
    pub async fn start_load(
        self: &Arc<Self>,
        desired_id: String,
        source: DownloadSource,
        downloader: &Arc<crate::engine::llm_downloader::LlmDownloader>,
    ) {
        // Cancel any existing load
        self.cancel_load().await;

        let slot = Arc::clone(self);
        let downloader = Arc::clone(downloader);
        let handle = tokio::spawn(async move {
            let info = match available_local_llms()
                .into_iter()
                .find(|m| m.id == desired_id)
            {
                Some(i) => i,
                None => {
                    let mut s = slot.load_status.write().await;
                    *s = LoadStatus::Error {
                        llm_id: desired_id.clone(),
                        message: format!("unknown model: {desired_id}"),
                    };
                    return;
                }
            };

            // Download GGUF if not present
            if !downloader.has_gguf(&info) {
                {
                    let mut s = slot.load_status.write().await;
                    *s = LoadStatus::Downloading {
                        llm_id: desired_id.clone(),
                        progress: 0.0,
                    };
                }

                let (progress_tx, mut progress_rx) = tokio::sync::watch::channel(0.0f32);

                // Spawn progress monitor
                let status_ref = Arc::clone(&slot.load_status);
                let id_clone = desired_id.clone();
                let monitor = tokio::spawn(async move {
                    while progress_rx.changed().await.is_ok() {
                        let progress = *progress_rx.borrow();
                        let mut s = status_ref.write().await;
                        if matches!(&*s, LoadStatus::Downloading { .. }) {
                            *s = LoadStatus::Downloading {
                                llm_id: id_clone.clone(),
                                progress,
                            };
                        } else {
                            break;
                        }
                    }
                });

                let download_result = downloader.download_gguf(&info, source, progress_tx).await;
                monitor.abort();

                if let Err(e) = download_result {
                    // Clean up partial download
                    downloader.cleanup_partial(&info).await;
                    let mut s = slot.load_status.write().await;
                    *s = LoadStatus::Error {
                        llm_id: desired_id.clone(),
                        message: e.to_string(),
                    };
                    return;
                }
            }

            // Load the model
            let _ = slot.get_or_load(&desired_id).await;
        });
        *self.load_handle.lock().await = Some(handle);
    }

    /// Cancel any in-progress load.
    /// During download: cancels the async reqwest stream.
    /// During model loading: best-effort — the FFI load may complete in background.
    pub async fn cancel_load(&self) {
        if let Some(handle) = self.load_handle.lock().await.take() {
            handle.abort();
        }
        let mut s = self.load_status.write().await;
        match &*s {
            LoadStatus::Downloading { .. } | LoadStatus::Quantizing { .. } | LoadStatus::Loading { .. } => {
                *s = LoadStatus::Idle;
                tracing::info!("Model load cancelled");
            }
            _ => {}
        }
    }

    pub async fn unload(&self) {
        let mut guard = self.current.lock().await;
        if guard.is_some() {
            *guard = None;
            let mut s = self.load_status.write().await;
            *s = LoadStatus::Idle;
            tracing::info!("Engine unloaded");
        }
    }

    pub async fn loaded_id(&self) -> Option<String> {
        self.current
            .lock()
            .await
            .as_ref()
            .map(|e| e.loaded_id().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn slot_starts_empty() {
        let tmp = TempDir::new().unwrap();
        let slot = EngineSlot::new(tmp.path().to_path_buf());
        assert!(slot.loaded_id().await.is_none());
    }

    #[tokio::test]
    async fn unload_when_empty_is_noop() {
        let tmp = TempDir::new().unwrap();
        let slot = EngineSlot::new(tmp.path().to_path_buf());
        slot.unload().await;
        assert!(slot.loaded_id().await.is_none());
    }

    #[tokio::test]
    async fn get_or_load_unknown_model_fails() {
        let tmp = TempDir::new().unwrap();
        let slot = EngineSlot::new(tmp.path().to_path_buf());
        let result = slot.get_or_load("does-not-exist").await;
        assert!(matches!(result, Err(LocalLlmError::UnknownModel(_))));
    }

    #[tokio::test]
    async fn get_or_load_valid_model_not_downloaded() {
        let tmp = TempDir::new().unwrap();
        let slot = EngineSlot::new(tmp.path().to_path_buf());
        let result = slot.get_or_load("qwen3.5-0.8b").await;
        assert!(matches!(result, Err(LocalLlmError::NotDownloaded)));
    }
}
