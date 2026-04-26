//! Translation prompt + response parsing for `LlmRouter::translate_lines`.
//!
//! We send the LLM a JSON array of `{id, text}` and expect back a JSON
//! array of `{id, text}` with translations. Order preservation is asked
//! for in the prompt but not relied on — we map by id.

use serde::{Deserialize, Serialize};

use crate::engine::llm_prompt::ChatMessage;

/// We treat the line id as an opaque string end-to-end. The transcript
/// pipeline currently happens to mint UUIDs, but the backend never uses
/// the id semantically — it just echoes it back to the frontend, which
/// matches against its own `TranscriptLine.id`. Keeping it `String`
/// avoids 422-on-deserialize if a caller (e.g. a future synthetic id
/// from `appendLiveTranscript`) sends a non-UUID id.
#[derive(Debug, Clone, Serialize)]
pub struct TranslateLineRequest {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslateLineResponse {
    pub id: String,
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
Reply with the JSON object and nothing else. Do NOT include reasoning, \
analysis, verification steps, or process notes. Do NOT use <think> \
tags. Do NOT prefix the output with prose like \"Here is the \
translation:\". Output starts with `{` and ends with `}`.\n\
\n\
Schema:\n\
{\"translations\": [{\"id\": \"...\", \"text\": \"...\"}, ...]}\n\
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

/// Parse the LLM response. Tolerates a wide range of off-spec output:
///   * `<think>...</think>` reasoning preambles
///   * Markdown code fences (```json ... ```)
///   * Prompt-echoed examples — we keep the LAST valid block found
///   * The canonical envelope `{"translations": [...]}`
///   * Bare arrays `[{"id":...,"text":...}, ...]` (model dropped the wrapper)
///   * Trailing junk after the JSON (e.g. `]}]` tails seen in the wild)
pub fn parse_translate_response(
    raw: &str,
) -> Result<Vec<TranslateLineResponse>, TranslateParseError> {
    let stripped = strip_think_tags(raw);
    let stripped = strip_code_fences(&stripped);

    let mut best: Option<Vec<TranslateLineResponse>> = None;

    // Pass 1: walk balanced `{...}` blocks and keep the last that
    // parses as the canonical envelope.
    for candidate in balanced_blocks(stripped, '{', '}') {
        if let Ok(env) = serde_json::from_str::<TranslateBatchEnvelope>(&candidate) {
            best = Some(env.translations);
        }
    }

    // Pass 2: if no envelope matched, accept a bare `[{id, text}, ...]`
    // array. The model occasionally drops the `{"translations": ...}`
    // wrapper (especially when it gets truncated or wraps in a code
    // fence).
    if best.is_none() {
        for candidate in balanced_blocks(stripped, '[', ']') {
            if let Ok(items) = serde_json::from_str::<Vec<TranslateLineResponse>>(&candidate) {
                best = Some(items);
            }
        }
    }

    best.ok_or(TranslateParseError::NoTranslationsFound)
}

/// Iterate every top-level balanced block delimited by `open`/`close`
/// chars, in left-to-right order. Skips delimiters inside JSON strings
/// (with backslash escape) so a quoted `"}"` doesn't end the block.
fn balanced_blocks(s: &str, open: char, close: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some(&(i, ch)) = chars.peek() {
        if ch != open {
            chars.next();
            continue;
        }
        // Walk this block.
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        let mut end = None;
        for (j, c) in s[i..].char_indices() {
            if escape {
                escape = false;
                continue;
            }
            if in_string {
                if c == '\\' {
                    escape = true;
                } else if c == '"' {
                    in_string = false;
                }
                continue;
            }
            match c {
                '"' => in_string = true,
                c if c == open => depth += 1,
                c if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(i + j + c.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(e) = end {
            out.push(s[i..e].to_string());
            // Resume after this block.
            while let Some(&(k, _)) = chars.peek() {
                if k >= e {
                    break;
                }
                chars.next();
            }
        } else {
            // Unbalanced — don't keep walking inside it.
            chars.next();
        }
    }
    out
}

fn strip_code_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    // ```json\n...\n``` or ```\n...\n```
    let after_open = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"));
    if let Some(rest) = after_open {
        let rest = rest.trim_start();
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
        // Unclosed fence — strip just the opener so the parser can still
        // walk balanced blocks in the remainder.
        return rest;
    }
    trimmed
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
                id: "line-1".into(),
                text: "hello".into(),
            },
        ];
        let msgs = build_translate_messages("zh-CN", &lines);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("Target language: zh-CN"));
        assert!(msgs[1].content.contains("\"hello\""));
        assert!(msgs[1].content.contains("line-1"));
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
    fn parse_strips_json_code_fence() {
        let raw = "```json\n{\"translations\":[{\"id\":\"line-1\",\"text\":\"你好\"}]}\n```";
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "你好");
    }

    #[test]
    fn parse_accepts_bare_array_fallback() {
        // The qwen3.5-2b failure pattern: model drops the `{"translations": …}`
        // wrapper and emits a bare array (sometimes inside a code fence).
        let raw = "```json\n[{\"id\":\"line-1\",\"text\":\"hello\"}]\n```";
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hello");
    }

    #[test]
    fn parse_tolerates_trailing_junk_after_array() {
        // Real production output: bare array followed by a junk `}]` tail.
        let raw = "```json\n[{\"id\":\"abc\",\"text\":\"first\"}]}]";
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "abc");
        assert_eq!(out[0].text, "first");
    }

    #[test]
    fn parse_string_with_close_brace_is_not_block_terminator() {
        // Make sure a `}` inside a JSON string doesn't prematurely close
        // the block during balanced-block walking.
        let raw = r#"{"translations":[{"id":"x","text":"contains } literal"}]}"#;
        let out = parse_translate_response(raw).unwrap();
        assert_eq!(out[0].text, "contains } literal");
    }

    #[test]
    fn parse_errors_when_empty() {
        let err = parse_translate_response("totally unrelated text").unwrap_err();
        assert!(matches!(err, TranslateParseError::NoTranslationsFound));
    }
}
