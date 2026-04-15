use std::sync::Arc;

use tracing::info;

use crate::engine::agent::repair;
use crate::engine::agent::task::{StructuredTask, TaskInput};
use crate::engine::llm_prompt::ChatMessage;
use crate::engine::local_llm_engine::{EngineSlot, EnginePriority, GenerationParams};
use crate::engine::remote_llm_client::RemoteLlmClient;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("local LLM error: {0}")]
    Local(#[from] crate::engine::local_llm_engine::LocalLlmError),
    #[error("remote LLM error: {0}")]
    Remote(#[from] crate::engine::remote_llm_client::RemoteLlmError),
    #[error("JSON parse error: {0}")]
    Parse(String),
    #[error("JSON repair failed: {0}")]
    Repair(#[from] repair::RepairError),
    #[error("schema generation error: {0}")]
    Schema(String),
}

impl From<serde_json::Error> for AgentError {
    fn from(e: serde_json::Error) -> Self {
        AgentError::Parse(e.to_string())
    }
}

pub enum AgentBackend {
    Local {
        slot: Arc<EngineSlot>,
        model_id: String,
    },
    Remote(Arc<RemoteLlmClient>),
}

pub struct AgentPipeline {
    backend: AgentBackend,
}

impl AgentPipeline {
    pub fn new(backend: AgentBackend) -> Self {
        Self { backend }
    }

    /// Execute a structured extraction task and return typed output.
    pub async fn run<T: StructuredTask>(
        &self,
        task: &T,
        input: TaskInput,
    ) -> Result<T::Output, AgentError> {
        // Generate JSON Schema from Output type
        let gen = schemars::gen::SchemaSettings::draft07().into_generator();
        let schema = gen.into_root_schema_for::<T::Output>();
        let schema_str = serde_json::to_string(&schema)
            .map_err(|e| AgentError::Schema(e.to_string()))?;

        let config = task.config();
        let messages = build_messages(task, &input, &schema_str, matches!(&self.backend, AgentBackend::Remote(_)));

        info!(
            task_type = std::any::type_name::<T>(),
            msg_count = messages.len(),
            max_tokens = config.max_tokens,
            temperature = config.temperature,
            backend = match &self.backend {
                AgentBackend::Local { model_id, .. } => model_id.as_str(),
                AgentBackend::Remote(_) => "remote",
            },
            "agent: running structured task"
        );

        match &self.backend {
            AgentBackend::Local { slot, model_id } => {
                let engine = slot.get_or_load(model_id).await?;

                let params = GenerationParams {
                    max_tokens: config.max_tokens,
                    temperature: config.temperature,
                    json_mode: true,
                };

                let raw = engine
                    .chat_completion(messages, params, EnginePriority::Internal)
                    .await?;

                // Strip think tags if present
                let cleaned = repair::strip_think_tags(&raw);

                info!(raw_len = cleaned.len(), "agent: local structured output received");

                // Parse with repair fallback
                repair::parse_or_repair::<T::Output>(&cleaned).map_err(AgentError::from)
            }
            AgentBackend::Remote(client) => {
                let params = GenerationParams {
                    max_tokens: config.max_tokens,
                    temperature: config.temperature,
                    json_mode: true,
                };

                let raw = client
                    .chat_completion_raw(&messages, &params)
                    .await?;

                info!(raw_len = raw.len(), "agent: remote output received, attempting repair parse");

                // Remote path: repair + parse
                repair::parse_or_repair::<T::Output>(&raw).map_err(AgentError::from)
            }
        }
    }
}

/// Build chat messages for the task. For remote backends, inject the JSON
/// schema into the system prompt so the model knows the expected format.
fn build_messages(
    task: &impl StructuredTask,
    input: &TaskInput,
    schema_str: &str,
    inject_schema: bool,
) -> Vec<ChatMessage> {
    let system = if inject_schema {
        format!(
            "{}\n\nYou MUST respond with valid JSON matching this schema:\n{}",
            task.system_prompt(),
            schema_str,
        )
    } else {
        task.system_prompt()
    };

    vec![
        ChatMessage {
            role: "system".into(),
            content: system,
        },
        ChatMessage {
            role: "user".into(),
            content: task.user_prompt(input),
        },
    ]
}
