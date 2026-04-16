use reqwest::Client;
use serde::Deserialize;
use tracing::{error, info, warn};

use crate::config::LlmConfig;
use crate::engine::llm_prompt::build_todo_messages;

#[derive(Debug, thiserror::Error)]
pub enum RemoteLlmError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON parse failed: {0}")]
    Parse(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("LLM returned empty or invalid response")]
    InvalidResponse,
}

#[derive(Deserialize)]
struct LlmChoice {
    message: LlmMessage,
}

#[derive(Deserialize)]
struct LlmMessage {
    content: String,
}

#[derive(Deserialize)]
struct LlmChatResponse {
    choices: Vec<LlmChoice>,
}

#[derive(Debug, Deserialize)]
pub struct LlmTodoResponse {
    pub todos: Vec<LlmTodoItem>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmTodoItem {
    /// Short title (≤ 60 chars).
    #[serde(default)]
    pub title: Option<String>,
    /// Detailed description.
    pub description: String,
    pub priority: Option<String>,
    /// Local datetime string (e.g. "2026-04-17T10:00") — no timezone suffix.
    /// The backend converts to UTC using the system's local timezone.
    pub due_time: Option<String>,
    /// Label names picked from the available set fed in the prompt.
    #[serde(default)]
    pub labels: Vec<String>,
}

impl LlmTodoItem {
    /// Normalize fields: convert model-output strings like "None", "null", "N/A"
    /// to actual `None` so they aren't persisted as literal strings.
    /// Normalize junk values, validate and fix each field.
    pub fn validate_and_fix(mut self) -> Self {
        let none_values = ["None", "none", "null", "N/A", "n/a", "unknown", ""];

        // title: trim, clear junk
        self.title = self.title
            .filter(|v| !none_values.contains(&v.as_str()))
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());

        // description: trim
        self.description = self.description.trim().to_string();

        // priority: normalize to lowercase, accept common variants
        self.priority = self.priority
            .filter(|v| !none_values.contains(&v.as_str()))
            .and_then(|p| match p.trim().to_lowercase().as_str() {
                "high" | "h" | "urgent" | "critical" => Some("high".into()),
                "medium" | "med" | "m" | "normal" | "moderate" => Some("medium".into()),
                "low" | "l" | "minor" => Some("low".into()),
                _ => None,
            });

        // due_time: try multiple formats, keep only if parseable as NaiveDateTime
        self.due_time = self.due_time
            .filter(|v| !none_values.contains(&v.as_str()))
            .and_then(|dt| {
                let s = dt.trim();
                let formats = [
                    "%Y-%m-%dT%H:%M",
                    "%Y-%m-%dT%H:%M:%S",
                    "%Y-%m-%d %H:%M",
                    "%Y-%m-%d %H:%M:%S",
                    "%m/%d/%Y %H:%M",
                    "%m/%d/%Y",
                    "%Y-%m-%d",
                ];
                for fmt in &formats {
                    if chrono::NaiveDateTime::parse_from_str(s, fmt).is_ok() {
                        return Some(s.to_string());
                    }
                }
                // Date-only → default to 09:00
                if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
                    return Some(format!("{s}T09:00"));
                }
                if chrono::NaiveDate::parse_from_str(s, "%m/%d/%Y").is_ok() {
                    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%m/%d/%Y") {
                        return Some(d.format("%Y-%m-%dT09:00").to_string());
                    }
                }
                None
            });

        // labels: trim, remove junk
        self.labels.retain(|v| !none_values.contains(&v.trim()));
        self.labels.iter_mut().for_each(|l| *l = l.trim().to_string());

        self
    }

    /// A result is usable if it has a non-empty description.
    pub fn is_usable(&self) -> bool {
        !self.description.is_empty()
    }
}

impl std::fmt::Display for LlmTodoItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.description)
    }
}

pub struct RemoteLlmClient {
    client: Client,
    config: LlmConfig,
}

impl RemoteLlmClient {
    pub fn new(config: LlmConfig) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build reqwest client");

        Self { client, config }
    }

    pub async fn generate_todos(
        &self,
        transcript: &str,
        label_names: &[String],
    ) -> Result<Vec<LlmTodoItem>, RemoteLlmError> {
        info!(transcript_len = transcript.len(), "Calling remote LLM for todo generation");

        let messages = build_todo_messages(transcript, label_names);
        let openai_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect();

        let payload = serde_json::json!({
            "model": self.config.model,
            "messages": openai_messages,
            "response_format": {"type": "json_object"},
            "temperature": 0.1,
            "max_tokens": 2000,
        });

        let base = self.config.base_url.trim_end_matches('/');
        let url = format!("{base}/chat/completions");

        let mut attempt = 0;
        let max_attempts = 2;

        loop {
            attempt += 1;
            match self.client
                .post(&url)
                .bearer_auth(&self.config.api_key)
                .json(&payload)
                .send()
                .await
            {
                Ok(resp) => {
                    if resp.status().is_server_error() && attempt < max_attempts {
                        warn!(attempt, "Remote LLM returned 5xx, retrying");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                    match resp.json::<LlmChatResponse>().await {
                        Ok(chat_resp) => {
                            let content = chat_resp.choices.first()
                                .map(|c| &c.message.content)
                                .ok_or(RemoteLlmError::InvalidResponse)?;
                            tracing::info!(raw_json = %content, "Remote LLM raw response");
                            let todos: LlmTodoResponse = serde_json::from_str(content)
                                .map_err(|e| RemoteLlmError::Parse(e.into()))?;
                            let todos: Vec<_> = todos.todos.into_iter().map(|t| t.validate_and_fix()).collect();
                            info!(count = todos.len(), "Remote LLM returned todo items");
                            return Ok(todos);
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to parse remote LLM response as JSON");
                            return Err(RemoteLlmError::Http(e));
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, attempt, "Remote LLM HTTP request failed");
                    if e.is_timeout() && attempt < max_attempts {
                        warn!(attempt, "Timeout, retrying");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                    return Err(RemoteLlmError::Http(e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_response() {
        let json = r#"{"todos": [{"description": "Review budget", "assigned_to": "Alice", "priority": "high", "speaker_name": "Alice"}]}"#;
        let result: LlmTodoResponse = serde_json::from_str(json).unwrap();
        assert_eq!(result.todos.len(), 1);
        assert_eq!(result.todos[0].description, "Review budget");
        assert_eq!(result.todos[0].priority.as_deref(), Some("high"));
    }

    #[test]
    fn test_parse_empty_response() {
        let json = r#"{"todos": []}"#;
        let result: LlmTodoResponse = serde_json::from_str(json).unwrap();
        assert!(result.todos.is_empty());
    }

    #[test]
    fn test_parse_malformed_response() {
        let json = r#"not json"#;
        let result: Result<LlmTodoResponse, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
