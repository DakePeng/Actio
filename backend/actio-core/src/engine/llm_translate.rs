//! Translation prompt + response parsing for `LlmRouter::translate_lines`.
//!
//! We send the LLM a JSON array of `{id, text}` and expect back a JSON
//! array of `{id, text}` with translations. Order preservation is asked
//! for in the prompt but not relied on — we map by id.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::llm_prompt::ChatMessage;

#[derive(Debug, Clone, Serialize)]
pub struct TranslateLineRequest {
    pub id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslateLineResponse {
    pub id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TranslateBatchEnvelope {
    translations: Vec<TranslateLineResponse>,
}

const SYSTEM_PROMPT: &str = "\
You are a translation assistant. You will receive a JSON array of \
transcript lines, each with an `id` and `text`. Translate each `text` \
into the requested target language. If a line is already in the \
target language, return it verbatim. Preserve speaker tone and \
punctuation. Do not add commentary or notes.\n\
\n\
Output ONLY a single JSON object — no markdown, no fences, no \
explanation:\n\
{\"translations\": [{\"id\": \"...uuid...\", \"text\": \"...\"}, ...]}\n\
\n\
The `translations` array MUST contain one entry per input id, in the \
same order. Do not omit, merge, or split lines.";

pub fn build_translate_messages(
    target_lang: &str,
    lines: &[TranslateLineRequest],
) -> Vec<ChatMessage> {
    let lines_json = serde_json::to_string(lines).expect("Vec<TranslateLineRequest> serialises");
    let user = format!("Target language: {target_lang}\n\nLines:\n{lines_json}");
    vec![
        ChatMessage {
            role: "system".into(),
            content: SYSTEM_PROMPT.to_string(),
        },
        ChatMessage {
            role: "user".into(),
            content: user,
        },
    ]
}

/// Parse the LLM response. Tolerates `</think>`-style preambles and
/// prompt-echoed JSON by walking all balanced `{...}` blocks and
/// keeping the LAST one that contains a valid `translations` array.
pub fn parse_translate_response(
    raw: &str,
) -> Result<Vec<TranslateLineResponse>, TranslateParseError> {
    let stripped = strip_think_tags(raw);

    let mut best: Option<Vec<TranslateLineResponse>> = None;
    let mut depth: i32 = 0;
    let mut start: Option<usize> = None;
    for (i, ch) in stripped.char_indices() {
        match ch {
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        let candidate = &stripped[s..=i];
                        if let Ok(env) =
                            serde_json::from_str::<TranslateBatchEnvelope>(candidate)
                        {
                            best = Some(env.translations);
                        }
                    }
                    start = None;
                }
            }
            _ => {}
        }
    }

    best.ok_or(TranslateParseError::NoTranslationsFound)
}

fn strip_think_tags(raw: &str) -> String {
    if let Some(end) = raw.rfind("</think>") {
        return raw[end + "</think>".len()..].trim_start().to_string();
    }
    raw.to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum TranslateParseError {
    #[error("no `translations` array found in LLM response")]
    NoTranslationsFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_messages_includes_target_lang_and_lines_json() {
        let lines = vec![
            TranslateLineRequest {
                id: Uuid::nil(),
                text: "hello".into(),
            },
        ];
        let msgs = build_translate_messages("zh-CN", &lines);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("Target language: zh-CN"));
        assert!(msgs[1].content.contains("\"hello\""));
        assert!(msgs[1].content.contains(&Uuid::nil().to_string()));
    }

    #[test]
    fn parse_canonical_response() {
        let raw = r#"{"translations":[{"id":"00000000-0000-0000-0000-000000000001","text":"你好"}]}"#;
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "你好");
    }

    #[test]
    fn parse_with_think_tag_preamble() {
        let raw = r#"<think>let me think</think>{"translations":[{"id":"00000000-0000-0000-0000-000000000001","text":"你好"}]}"#;
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "你好");
    }

    #[test]
    fn parse_with_prose_preamble() {
        let raw = "Sure, here are the translations:\n\n{\"translations\":[{\"id\":\"00000000-0000-0000-0000-000000000001\",\"text\":\"你好\"}]}";
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "你好");
    }

    #[test]
    fn parse_picks_last_block_when_prompt_is_echoed() {
        let raw = "Example: {\"translations\":[{\"id\":\"00000000-0000-0000-0000-000000000000\",\"text\":\"example\"}]}\nActual: {\"translations\":[{\"id\":\"00000000-0000-0000-0000-000000000001\",\"text\":\"real\"}]}";
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "real");
    }

    #[test]
    fn parse_errors_when_empty() {
        let err = parse_translate_response("totally unrelated text").unwrap_err();
        assert!(matches!(err, TranslateParseError::NoTranslationsFound));
    }
}
