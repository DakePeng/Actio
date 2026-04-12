use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;

use crate::engine::llm_catalog::{available_local_llms, LocalLlmInfo};
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
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            max_tokens: 2000,
            temperature: 0.1,
            json_mode: false,
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
pub struct LocalLlmEngine {
    loaded_id: String,
    metadata: LocalLlmInfo,
    model: Arc<mistralrs::Model>,
    waiting_internal: std::sync::atomic::AtomicUsize,
}

#[cfg(feature = "local-llm")]
impl LocalLlmEngine {
    pub fn load_blocking(model_dir: &Path, info: &LocalLlmInfo) -> Result<Self, LocalLlmError> {
        let gguf_path = model_dir
            .join("llms")
            .join(&info.id)
            .join(&info.gguf_filename);
        if !gguf_path.exists() {
            return Err(LocalLlmError::NotDownloaded);
        }
        info!(model_id = %info.id, path = %gguf_path.display(), "Loading local LLM");

        // Use tokio runtime for the async GgufModelBuilder.
        // This function is called from spawn_blocking, so we create a
        // temporary runtime for the async build call.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| LocalLlmError::LoadFailed(e.to_string()))?;

        let hf_repo = info.hf_repo.clone();
        let gguf_filename = info.gguf_filename.clone();
        let tok_model_id = format!("Qwen/{}", if info.id.contains("0.8b") {
            "Qwen3.5-0.8B"
        } else {
            "Qwen3.5-2B"
        });

        let model = rt.block_on(async {
            mistralrs::GgufModelBuilder::new(hf_repo, vec![gguf_filename])
                .with_tok_model_id(tok_model_id)
                .with_logging()
                .build()
                .await
        }).map_err(|e| classify_load_error(&info.id, info.ram_mb, e))?;

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

        if priority == EnginePriority::Internal {
            self.waiting_internal.fetch_add(1, Ordering::SeqCst);
        }

        if priority == EnginePriority::External {
            for _ in 0..5 {
                if self.waiting_internal.load(Ordering::SeqCst) == 0 {
                    break;
                }
                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        let mut text_messages = mistralrs::TextMessages::new();
        for msg in &messages {
            let role = match msg.role.as_str() {
                "system" => mistralrs::TextMessageRole::System,
                "user" => mistralrs::TextMessageRole::User,
                "assistant" => mistralrs::TextMessageRole::Assistant,
                _ => mistralrs::TextMessageRole::User,
            };
            text_messages = text_messages.add_message(role, &msg.content);
        }

        if priority == EnginePriority::Internal {
            self.waiting_internal.fetch_sub(1, Ordering::SeqCst);
        }

        let response = self.model
            .send_chat_request(text_messages)
            .await
            .map_err(|e| LocalLlmError::InferenceFailed(e.to_string()))?;

        let content = response.choices.first()
            .and_then(|c| c.message.content.as_ref())
            .cloned()
            .unwrap_or_default();

        Ok(content)
    }

    pub fn loaded_id(&self) -> &str {
        &self.loaded_id
    }

    pub fn metadata(&self) -> &LocalLlmInfo {
        &self.metadata
    }
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
    pub fn load_blocking(_model_dir: &Path, _info: &LocalLlmInfo) -> Result<Self, LocalLlmError> {
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
// EngineSlot — lazy-sticky lifecycle holder
// ---------------------------------------------------------------------------

pub struct EngineSlot {
    model_dir: PathBuf,
    current: Mutex<Option<Arc<LocalLlmEngine>>>,
}

impl EngineSlot {
    pub fn new(model_dir: PathBuf) -> Self {
        Self {
            model_dir,
            current: Mutex::new(None),
        }
    }

    pub async fn get_or_load(&self, desired_id: &str) -> Result<Arc<LocalLlmEngine>, LocalLlmError> {
        let mut guard = self.current.lock().await;
        if let Some(engine) = guard.as_ref() {
            if engine.loaded_id() == desired_id {
                return Ok(Arc::clone(engine));
            }
            // Drop old before loading new — releases RAM first.
            *guard = None;
        }
        let info = available_local_llms()
            .into_iter()
            .find(|m| m.id == desired_id)
            .ok_or_else(|| LocalLlmError::UnknownModel(desired_id.into()))?;

        let model_dir = self.model_dir.clone();
        let info_clone = info.clone();
        let engine = tokio::task::spawn_blocking(move || {
            LocalLlmEngine::load_blocking(&model_dir, &info_clone)
        })
        .await
        .map_err(|e| LocalLlmError::LoadFailed(e.to_string()))??;
        let engine = Arc::new(engine);
        *guard = Some(Arc::clone(&engine));
        Ok(engine)
    }

    pub async fn unload(&self) {
        let mut guard = self.current.lock().await;
        if guard.is_some() {
            *guard = None;
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
    async fn get_or_load_not_downloaded_fails() {
        let tmp = TempDir::new().unwrap();
        let slot = EngineSlot::new(tmp.path().to_path_buf());
        let result = slot.get_or_load("qwen3.5-0.8b-q4km").await;
        assert!(matches!(
            result,
            Err(LocalLlmError::NotDownloaded) | Err(LocalLlmError::FeatureDisabled)
        ));
    }
}
