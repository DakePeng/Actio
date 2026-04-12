use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::engine::llm_prompt::build_todo_messages;
use crate::engine::local_llm_engine::{EngineSlot, EnginePriority, GenerationParams, LocalLlmError};
use crate::engine::remote_llm_client::{LlmTodoItem, LlmTodoResponse, RemoteLlmClient, RemoteLlmError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LlmSelection {
    Disabled,
    Local { id: String },
    Remote,
}

impl Default for LlmSelection {
    fn default() -> Self {
        LlmSelection::Disabled
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LlmRouterError {
    #[error("no LLM backend selected")]
    Disabled,
    #[error(transparent)]
    Local(#[from] LocalLlmError),
    #[error(transparent)]
    Remote(#[from] RemoteLlmError),
    #[error("failed to parse LLM response as JSON: {0}")]
    Parse(String),
}

pub enum LlmRouter {
    Disabled,
    Local {
        slot: Arc<EngineSlot>,
        model_id: String,
    },
    Remote(Arc<RemoteLlmClient>),
}

impl LlmRouter {
    pub fn is_local(&self) -> bool {
        matches!(self, LlmRouter::Local { .. })
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, LlmRouter::Disabled)
    }

    pub async fn generate_todos(
        &self,
        transcript: &str,
    ) -> Result<Vec<LlmTodoItem>, LlmRouterError> {
        match self {
            LlmRouter::Disabled => Ok(vec![]),
            LlmRouter::Remote(client) => client
                .generate_todos(transcript)
                .await
                .map_err(LlmRouterError::Remote),
            LlmRouter::Local { slot, model_id } => {
                let engine = slot
                    .get_or_load(model_id)
                    .await
                    .map_err(LlmRouterError::Local)?;
                let messages = build_todo_messages(transcript);
                let json = engine
                    .chat_completion(
                        messages,
                        GenerationParams {
                            max_tokens: 2000,
                            temperature: 0.1,
                            json_mode: true,
                        },
                        EnginePriority::Internal,
                    )
                    .await
                    .map_err(LlmRouterError::Local)?;
                let parsed: LlmTodoResponse = parse_with_fallback(&json)?;
                Ok(parsed.todos)
            }
        }
    }
}

fn parse_with_fallback(raw: &str) -> Result<LlmTodoResponse, LlmRouterError> {
    if let Ok(parsed) = serde_json::from_str::<LlmTodoResponse>(raw) {
        return Ok(parsed);
    }
    if let Some(start) = raw.find('{') {
        if let Some(end) = raw.rfind('}') {
            if end > start {
                if let Ok(parsed) = serde_json::from_str::<LlmTodoResponse>(&raw[start..=end]) {
                    return Ok(parsed);
                }
            }
        }
    }
    // Log metadata only — not the full raw response (privacy: spec rev 2, finding #10).
    tracing::warn!(
        response_len = raw.len(),
        response_prefix = %raw.chars().take(50).collect::<String>(),
        "Local LLM returned unparseable JSON, returning empty todos"
    );
    Ok(LlmTodoResponse { todos: vec![] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_default_is_disabled() {
        assert_eq!(LlmSelection::default(), LlmSelection::Disabled);
    }

    #[test]
    fn selection_serializes_with_kind_tag() {
        let local = LlmSelection::Local {
            id: "qwen3.5-0.8b-q4km".into(),
        };
        let json = serde_json::to_string(&local).unwrap();
        assert!(json.contains("\"kind\":\"local\""));
        assert!(json.contains("\"id\":\"qwen3.5-0.8b-q4km\""));
    }

    #[test]
    fn parse_with_fallback_handles_pure_json() {
        let raw = r#"{"todos":[{"description":"x"}]}"#;
        let parsed = parse_with_fallback(raw).unwrap();
        assert_eq!(parsed.todos.len(), 1);
    }

    #[test]
    fn parse_with_fallback_handles_wrapped_json() {
        let raw = r#"Here are the todos: {"todos":[{"description":"x"}]} done."#;
        let parsed = parse_with_fallback(raw).unwrap();
        assert_eq!(parsed.todos.len(), 1);
    }

    #[test]
    fn parse_with_fallback_returns_empty_on_garbage() {
        let raw = "totally not json at all";
        let parsed = parse_with_fallback(raw).unwrap();
        assert!(parsed.todos.is_empty());
    }

    #[tokio::test]
    async fn disabled_returns_empty_todos() {
        let router = LlmRouter::Disabled;
        let todos = router.generate_todos("anything").await.unwrap();
        assert!(todos.is_empty());
    }
}
