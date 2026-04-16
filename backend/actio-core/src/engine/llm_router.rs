use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::engine::llm_prompt::{build_todo_messages, build_retry_messages};
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
        label_names: &[String],
    ) -> Result<Vec<LlmTodoItem>, LlmRouterError> {
        match self {
            LlmRouter::Disabled => Ok(vec![]),
            LlmRouter::Remote(client) => {
                let items = client
                    .generate_todos(transcript, label_names)
                    .await
                    .map_err(LlmRouterError::Remote)?;
                Ok(items.into_iter().map(|t| t.validate_and_fix()).collect())
            }
            LlmRouter::Local { slot, model_id } => {
                let engine = slot
                    .get_or_load(model_id)
                    .await
                    .map_err(LlmRouterError::Local)?;

                const MAX_ATTEMPTS: usize = 3;
                let params = GenerationParams {
                    max_tokens: 2000,
                    temperature: 0.1,
                    json_mode: true,
                    thinking_budget: Some((transcript.len() / 10).clamp(100, 500)),
                };

                let mut last_raw = String::new();
                for attempt in 1..=MAX_ATTEMPTS {
                    let messages = if attempt == 1 {
                        build_todo_messages(transcript, label_names)
                    } else {
                        tracing::warn!(attempt, "Retrying LLM extraction with correction prompt");
                        build_retry_messages(transcript, label_names, &last_raw)
                    };

                    let json = engine
                        .chat_completion(messages, params.clone(), EnginePriority::Internal)
                        .await
                        .map_err(LlmRouterError::Local)?;

                    tracing::info!(attempt, raw_json = %json, "Local LLM raw response");
                    let json_stripped = strip_think_tags(&json);
                    let parsed = parse_with_fallback(&json_stripped)?;

                    let items: Vec<LlmTodoItem> = parsed
                        .todos
                        .into_iter()
                        .map(|t| t.validate_and_fix())
                        .filter(|t| t.is_usable())
                        .collect();

                    if !items.is_empty() {
                        return Ok(items);
                    }

                    // Save raw output for the retry prompt
                    last_raw = json;
                }

                tracing::error!("LLM extraction failed after {MAX_ATTEMPTS} attempts");
                Err(LlmRouterError::Parse(format!(
                    "Could not extract a valid action item after {MAX_ATTEMPTS} attempts"
                )))
            }
        }
    }
}

fn strip_think_tags(raw: &str) -> String {
    let mut result = raw.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end_pos) = result.find("</think>") {
            let end = end_pos + "</think>".len();
            result = format!("{}{}", &result[..start], &result[end..]);
        } else {
            result = result[..start].to_string();
            break;
        }
    }
    result.trim().to_string()
}

/// Robustly parse LLM output into a list of `LlmTodoItem`.
///
/// Handles common model quirks:
/// - Markdown code fences (```json ... ```)
/// - Single object `{...}` (current default)
/// - Wrapped `{"todos": [...]}` or bare array `[...]` (legacy/fallback)
/// - Surrounding prose around the JSON block
fn parse_with_fallback(raw: &str) -> Result<LlmTodoResponse, LlmRouterError> {
    let stripped = strip_code_fences(raw);

    // 1. Try as single item (current prompt format)
    if let Ok(item) = serde_json::from_str::<LlmTodoItem>(stripped) {
        return Ok(LlmTodoResponse { todos: vec![item] });
    }

    // 2. Try as {"todos": [...]} wrapper
    if let Ok(parsed) = serde_json::from_str::<LlmTodoResponse>(stripped) {
        return Ok(parsed);
    }

    // 3. Try as bare array
    if let Ok(items) = serde_json::from_str::<Vec<LlmTodoItem>>(stripped) {
        return Ok(LlmTodoResponse { todos: items });
    }

    // 4. Extract JSON block from surrounding prose
    if let Some(json) = extract_json_block(stripped, '{', '}') {
        if let Ok(item) = serde_json::from_str::<LlmTodoItem>(&json) {
            return Ok(LlmTodoResponse { todos: vec![item] });
        }
        if let Ok(parsed) = serde_json::from_str::<LlmTodoResponse>(&json) {
            return Ok(parsed);
        }
    }
    if let Some(json) = extract_json_block(stripped, '[', ']') {
        if let Ok(items) = serde_json::from_str::<Vec<LlmTodoItem>>(&json) {
            return Ok(LlmTodoResponse { todos: items });
        }
    }

    tracing::warn!(
        response_len = raw.len(),
        response_prefix = %raw.chars().take(80).collect::<String>(),
        "Local LLM returned unparseable JSON, returning empty todos"
    );
    Ok(LlmTodoResponse { todos: vec![] })
}

/// Strip ```json ... ``` or ``` ... ``` fences from LLM output.
fn strip_code_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
    }
    trimmed
}

/// Find the outermost balanced block delimited by `open`/`close` chars.
fn extract_json_block(s: &str, open: char, close: char) -> Option<String> {
    let start = s.find(open)?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in s[start..].char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(s[start..start + i + 1].to_string());
            }
        }
    }
    None
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
            id: "qwen3.5-0.8b".into(),
        };
        let json = serde_json::to_string(&local).unwrap();
        assert!(json.contains("\"kind\":\"local\""));
        assert!(json.contains("\"id\":\"qwen3.5-0.8b\""));
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
