use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Serialize};

/// Image attached to an extraction request for multimodal context.
pub struct ImageContext {
    pub data: Vec<u8>,
    pub mime_type: String,
}

/// Input to a structured extraction task.
pub struct TaskInput {
    pub text: String,
    pub images: Vec<ImageContext>,
}

/// Configuration for task execution.
pub struct TaskConfig {
    pub max_tokens: usize,
    pub temperature: f32,
    /// If set, enables thinking mode. v1: `<think>` tags are stripped
    /// post-generation. Budget enforcement deferred to v1.1.
    pub thinking_budget: Option<usize>,
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self {
            max_tokens: 2000,
            temperature: 0.1,
            thinking_budget: None,
        }
    }
}

/// A structured extraction or transformation task.
///
/// Define a new task by implementing this trait. The `Output` type must derive
/// `Serialize`, `Deserialize`, and `JsonSchema` — the pipeline auto-generates
/// a JSON Schema from it and uses `llguidance` to constrain local model output
/// at the token level.
pub trait StructuredTask: Send + Sync {
    type Output: Serialize + DeserializeOwned + JsonSchema + Send;

    /// System prompt that instructs the model what to extract/transform.
    fn system_prompt(&self) -> String;

    /// Format the user message from task input. Override for task-specific
    /// formatting (e.g., wrapping transcript in XML tags).
    fn user_prompt(&self, input: &TaskInput) -> String {
        input.text.clone()
    }

    /// Execution parameters. Override to customize tokens, temperature, etc.
    fn config(&self) -> TaskConfig {
        TaskConfig::default()
    }
}
