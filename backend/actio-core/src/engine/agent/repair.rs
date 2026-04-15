use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    #[error("JSON parse failed after repair: {0}")]
    ParseFailed(String),
}

/// Strip markdown code fences (```json ... ``` or ``` ... ```) from LLM output.
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

/// Strip `<think>...</think>` tags from model output (reasoning budget v1).
pub fn strip_think_tags(raw: &str) -> String {
    let mut result = raw.to_string();
    while let Some(start) = result.find("<think>") {
        if let Some(end) = result.find("</think>") {
            let end = end + "</think>".len();
            result = format!("{}{}", &result[..start], &result[end..]);
        } else {
            // Unclosed <think> — strip from tag to end
            result = result[..start].to_string();
            break;
        }
    }
    result.trim().to_string()
}

/// Try to parse LLM output as typed JSON, with progressive repair fallbacks.
///
/// 1. Direct serde_json parse
/// 2. Strip markdown code fences, retry
/// 3. Use `llm_json` crate to repair malformed JSON, retry
pub fn parse_or_repair<T: DeserializeOwned>(raw: &str) -> Result<T, RepairError> {
    // 1. Direct parse
    if let Ok(v) = serde_json::from_str::<T>(raw) {
        return Ok(v);
    }

    // 2. Strip markdown fences
    let stripped = strip_code_fences(raw);
    if let Ok(v) = serde_json::from_str::<T>(stripped) {
        return Ok(v);
    }

    // 3. Repair malformed JSON via llm_json
    let options = llm_json::RepairOptions::default();
    let repaired = llm_json::repair_json(stripped, &options)
        .map_err(|e| RepairError::ParseFailed(format!("repair failed: {e}")))?;
    serde_json::from_str::<T>(&repaired)
        .map_err(|e| RepairError::ParseFailed(format!("{e} (after repair: {repaired:.100})")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Simple {
        name: String,
    }

    #[test]
    fn parse_valid_json() {
        let r: Simple = parse_or_repair(r#"{"name":"hello"}"#).unwrap();
        assert_eq!(r.name, "hello");
    }

    #[test]
    fn parse_markdown_fenced() {
        let r: Simple = parse_or_repair("```json\n{\"name\":\"hello\"}\n```").unwrap();
        assert_eq!(r.name, "hello");
    }

    #[test]
    fn parse_garbage_fails() {
        let r = parse_or_repair::<Simple>("totally not json at all");
        assert!(r.is_err());
    }

    #[test]
    fn strip_think_tags_basic() {
        assert_eq!(
            strip_think_tags("<think>reasoning here</think>answer"),
            "answer"
        );
    }

    #[test]
    fn strip_think_tags_none() {
        assert_eq!(strip_think_tags("no tags here"), "no tags here");
    }

    #[test]
    fn strip_think_tags_unclosed() {
        assert_eq!(strip_think_tags("prefix<think>dangling"), "prefix");
    }
}
