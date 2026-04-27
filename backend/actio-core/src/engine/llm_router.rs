use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::engine::llm_prompt::{build_retry_messages, build_todo_messages, build_window_messages};
use crate::engine::local_llm_engine::{
    EnginePriority, EngineSlot, GenerationParams, LocalLlmError,
};
use crate::engine::remote_llm_client::{
    LlmActionItem, LlmActionResponse, LlmTodoItem, LlmTodoResponse, RemoteLlmClient, RemoteLlmError,
};

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
    /// Test-only variant for integration tests. `action_items` feeds
    /// `generate_action_items_with_refs`; `translation_suffix` is
    /// appended to each input line by `translate_lines` (e.g. "[zh]")
    /// so order-preservation and id-mapping can be asserted.
    #[cfg(test)]
    Stub {
        action_items: Vec<LlmActionItem>,
        translation_suffix: String,
    },
}

impl LlmRouter {
    pub fn is_local(&self) -> bool {
        matches!(self, LlmRouter::Local { .. })
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, LlmRouter::Disabled)
    }

    /// Test-only constructor for the Stub variant.
    #[cfg(test)]
    pub fn stub(action_items: Vec<LlmActionItem>) -> Self {
        LlmRouter::Stub {
            action_items,
            translation_suffix: " [stub]".into(),
        }
    }

    /// Test-only constructor when translation behavior matters.
    #[cfg(test)]
    pub fn stub_with_translation_suffix(suffix: impl Into<String>) -> Self {
        LlmRouter::Stub {
            action_items: vec![],
            translation_suffix: suffix.into(),
        }
    }

    pub async fn generate_todos(
        &self,
        transcript: &str,
        label_names: &[String],
        image_data_urls: &[String],
    ) -> Result<Vec<LlmTodoItem>, LlmRouterError> {
        match self {
            LlmRouter::Disabled => Ok(vec![]),
            #[cfg(test)]
            LlmRouter::Stub { .. } => Ok(vec![]),
            LlmRouter::Remote(client) => {
                let items = client
                    .generate_todos(transcript, label_names, image_data_urls)
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
                // Suppress thinking. qwen3.5's chat template auto-opens
                // `<think>` on every assistant turn; even with the engine's
                // budget enforcement, the model has been observed to keep
                // emitting thinking-shaped prose past the force-closed
                // `</think>` tag, eating the whole max_tokens budget
                // without producing JSON. Closing the block immediately
                // is more reliable than capping reasoning length.
                let params = GenerationParams {
                    max_tokens: 2000,
                    temperature: 0.1,
                    json_mode: true,
                    thinking_budget: None,
                    suppress_thinking: true,
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

    /// Windowed action extraction. Returns items with `confidence`,
    /// `evidence_quote`, and `speaker_name` so the caller can gate into
    /// `status='open'` vs `'pending'` and attach the provenance the trace
    /// inspector needs. Unlike `generate_todos`, this path does NOT retry
    /// on an empty result — a quiet window legitimately produces no items.
    pub async fn generate_action_items_with_refs(
        &self,
        attributed_transcript: &str,
        label_names: &[String],
        window_local_date: &str,
        profile: Option<&crate::domain::types::TenantProfile>,
    ) -> Result<Vec<LlmActionItem>, LlmRouterError> {
        match self {
            LlmRouter::Disabled => Err(LlmRouterError::Disabled),
            #[cfg(test)]
            LlmRouter::Stub { action_items, .. } => Ok(action_items.clone()),
            LlmRouter::Remote(client) => client
                .generate_action_items_with_refs(
                    attributed_transcript,
                    label_names,
                    window_local_date,
                    profile,
                )
                .await
                .map_err(LlmRouterError::Remote),
            LlmRouter::Local { slot, model_id } => {
                let engine = slot
                    .get_or_load(model_id)
                    .await
                    .map_err(LlmRouterError::Local)?;
                // Same rationale as `generate_todos` — see comment there.
                // The action-item prompt is more structured (confidence
                // tier, evidence quote, speaker name) but qwen3.5-2b on
                // CPU is more reliable when forced to answer directly
                // than when given rope to reason.
                let params = GenerationParams {
                    max_tokens: 2000,
                    temperature: 0.1,
                    json_mode: true,
                    thinking_budget: None,
                    suppress_thinking: true,
                };
                let messages = build_window_messages(
                    attributed_transcript,
                    label_names,
                    window_local_date,
                    profile,
                );
                let json = engine
                    .chat_completion(messages, params, EnginePriority::Internal)
                    .await
                    .map_err(LlmRouterError::Local)?;
                let json_stripped = strip_think_tags(&json);
                tracing::info!(raw_json = %json_stripped, "Local LLM windowed raw response");
                let parsed = parse_action_items_with_fallback(&json_stripped)?;
                let items: Vec<_> = parsed
                    .items
                    .into_iter()
                    .map(|t| t.validate_and_fix())
                    .filter(|t| t.is_usable())
                    .collect();
                Ok(items)
            }
        }
    }

    /// Translate each input line to `target_lang`. Returns translations
    /// in the same order as the input. The Local and Remote backends
    /// dispatch to a structured prompt that returns a JSON envelope;
    /// see `engine::llm_translate`.
    pub async fn translate_lines(
        &self,
        target_lang: &str,
        lines: Vec<crate::engine::llm_translate::TranslateLineRequest>,
    ) -> Result<Vec<crate::engine::llm_translate::TranslateLineResponse>, LlmRouterError> {
        match self {
            LlmRouter::Disabled => Err(LlmRouterError::Disabled),
            #[cfg(test)]
            LlmRouter::Stub {
                translation_suffix, ..
            } => Ok(lines
                .into_iter()
                .map(|l| crate::engine::llm_translate::TranslateLineResponse {
                    id: l.id,
                    text: format!("{}{}", l.text, translation_suffix),
                })
                .collect()),
            LlmRouter::Remote(client) => client
                .translate_lines(target_lang, lines)
                .await
                .map_err(LlmRouterError::Remote),
            LlmRouter::Local { slot, model_id } => {
                let engine = slot
                    .get_or_load(model_id)
                    .await
                    .map_err(LlmRouterError::Local)?;
                // Translation is mechanical — no reasoning needed. Disabling
                // the thinking budget skips the <think>\n injection that
                // otherwise pushes models like qwen3.5 into a long
                // chain-of-thought, occasionally a degenerate repetition
                // loop that burns the whole token budget before emitting
                // the JSON.
                //
                // Output size is bounded by input length — Chinese→English
                // is roughly 2–3× tokens out per char in (CJK ≈ 1 token/
                // char, English ≈ 1.3 tokens/word), plus ~30 tokens of
                // JSON envelope overhead per line. The frontend caps
                // batches at 4 lines of typical-utterance length, so 1000
                // tokens is plenty; the previous 2000-token ceiling was
                // pure dead weight that just gave a runaway model more
                // rope.
                let total_chars: usize = lines.iter().map(|l| l.text.len()).sum();
                let max_tokens = (120 + total_chars * 3).min(1000);
                let params = GenerationParams {
                    max_tokens,
                    temperature: 0.1,
                    json_mode: true,
                    thinking_budget: None,
                    suppress_thinking: true,
                };
                let messages =
                    crate::engine::llm_translate::build_translate_messages(target_lang, &lines);
                let json = engine
                    .chat_completion(messages, params, EnginePriority::Internal)
                    .await
                    .map_err(LlmRouterError::Local)?;
                tracing::info!(raw_json = %json, "Local LLM translate raw response");
                let parsed = crate::engine::llm_translate::parse_translate_response(&json)
                    .map_err(|e| LlmRouterError::Parse(e.to_string()))?;
                Ok(parsed)
            }
        }
    }
}

/// Lenient parser for the windowed response shape. Accepts:
///   {"items":[...]} (canonical),
///   bare array [...],
///   a code-fenced version of either,
///   {"todos":[...]} as a legacy alias (reshaped into items).
fn parse_action_items_with_fallback(raw: &str) -> Result<LlmActionResponse, LlmRouterError> {
    let stripped = strip_code_fences(raw);

    if let Ok(parsed) = serde_json::from_str::<LlmActionResponse>(stripped) {
        return Ok(parsed);
    }
    if let Ok(items) = serde_json::from_str::<Vec<LlmActionItem>>(stripped) {
        return Ok(LlmActionResponse { items });
    }
    // Fallback: {"todos":[...]} — the legacy shape. Deserialize into the
    // generic Value, pluck "todos" if present, try LlmActionItem parsing.
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(stripped) {
        if let Some(arr) = val.get("todos").and_then(|v| v.as_array()) {
            let items: Vec<LlmActionItem> = arr
                .iter()
                .filter_map(|v| serde_json::from_value::<LlmActionItem>(v.clone()).ok())
                .collect();
            if !items.is_empty() {
                return Ok(LlmActionResponse { items });
            }
        }
    }

    // Models often echo prompt examples like `{"items": [...]}` in their
    // prose preamble. Walk every balanced `{...}` and `[...]` block and keep
    // the LAST one that parses — the real answer is almost always at the end.
    let mut best: Option<LlmActionResponse> = None;
    for json in all_json_blocks(stripped, '{', '}') {
        if let Ok(parsed) = serde_json::from_str::<LlmActionResponse>(&json) {
            best = Some(parsed);
        }
    }
    for json in all_json_blocks(stripped, '[', ']') {
        if let Ok(items) = serde_json::from_str::<Vec<LlmActionItem>>(&json) {
            best = Some(LlmActionResponse { items });
        }
    }
    if let Some(parsed) = best {
        return Ok(parsed);
    }

    tracing::warn!(
        response_len = raw.len(),
        response_prefix = %raw.chars().take(80).collect::<String>(),
        "Local LLM returned unparseable windowed JSON, returning empty items"
    );
    Ok(LlmActionResponse { items: vec![] })
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
    // Some models (e.g. qwen3.5-2b) emit a thinking preamble without an
    // opening <think> tag but still close it. Drop everything up to and
    // including the first dangling </think>.
    if let Some(end_pos) = result.find("</think>") {
        let end = end_pos + "</think>".len();
        result = result[end..].to_string();
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

/// Iterate every top-level balanced block delimited by `open`/`close` chars,
/// in left-to-right order. Skips delimiters inside strings.
fn all_json_blocks(s: &str, open: char, close: char) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] as char == open {
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escape = false;
            let mut end_idx = None;
            for (j, ch) in s[i..].char_indices() {
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
                        end_idx = Some(i + j + ch.len_utf8());
                        break;
                    }
                }
            }
            if let Some(end) = end_idx {
                out.push(s[i..end].to_string());
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
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

    #[test]
    fn strip_think_tags_removes_dangling_close_tag() {
        let raw = "Thinking out loud about the request.\n   </think>\n{\"items\":[]}";
        let stripped = strip_think_tags(raw);
        assert_eq!(stripped, "{\"items\":[]}");
    }

    #[test]
    fn parse_action_items_prefers_last_block_over_echoed_example() {
        // Simulates qwen3.5-2b echoing the prompt's example shape inside a
        // thinking preamble before emitting the real answer.
        let raw = r#"Thinking: the format is `{"items": [...]}`.
        </think>
        {"items":[{"title":"Buy milk","description":"On the way home","confidence":"high","evidence_quote":"buy milk","speaker_name":null}]}"#;
        let stripped = strip_think_tags(raw);
        let parsed = parse_action_items_with_fallback(&stripped).unwrap();
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title.as_deref(), Some("Buy milk"));
    }

    #[test]
    fn parse_action_items_falls_through_to_empty_on_garbage() {
        let raw = "totally not json, no braces either";
        let parsed = parse_action_items_with_fallback(raw).unwrap();
        assert!(parsed.items.is_empty());
    }

    #[tokio::test]
    async fn disabled_returns_empty_todos() {
        let router = LlmRouter::Disabled;
        let todos = router.generate_todos("anything", &[], &[]).await.unwrap();
        assert!(todos.is_empty());
    }

    use crate::engine::llm_translate::TranslateLineRequest;

    #[tokio::test]
    async fn translate_lines_disabled_returns_disabled_error() {
        let router = LlmRouter::Disabled;
        let lines = vec![TranslateLineRequest {
            id: "line-1".into(),
            text: "hello".into(),
        }];
        let err = router.translate_lines("zh-CN", lines).await.unwrap_err();
        assert!(matches!(err, LlmRouterError::Disabled));
    }

    #[tokio::test]
    async fn translate_lines_stub_appends_suffix_in_order() {
        let router = LlmRouter::stub_with_translation_suffix(" [zh]");
        let lines = vec![
            TranslateLineRequest {
                id: "line-1".into(),
                text: "first".into(),
            },
            TranslateLineRequest {
                id: "line-2".into(),
                text: "second".into(),
            },
        ];
        let out = router.translate_lines("zh-CN", lines).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, "line-1");
        assert_eq!(out[0].text, "first [zh]");
        assert_eq!(out[1].id, "line-2");
        assert_eq!(out[1].text, "second [zh]");
    }
}
