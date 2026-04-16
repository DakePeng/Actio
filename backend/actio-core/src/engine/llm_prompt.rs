use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub const SYSTEM_PROMPT: &str = "\
You are a task extraction assistant. Convert user input into a structured task.\n\
\n\
Output ONLY a single JSON object — no markdown, no fences, no explanation:\n\
{\"title\": \"...\", \"description\": \"...\", \"priority\": \"high|medium|low\", \"due_time\": \"YYYY-MM-DDTHH:MM\", \"labels\": [\"...\"]}\n\
\n\
Fields:\n\
- title: short task name, under 50 chars. Use the same language as the input.\n\
- description: full details — who, what, where, when, why. Expand abbreviations. Same language as input.\n\
- priority: \"high\" (urgent/deadline soon), \"medium\" (normal), \"low\" (whenever). Default to \"medium\".\n\
- due_time: local time as \"YYYY-MM-DDTHH:MM\". Resolve \"tomorrow\" from today's date. Omit if no time reference.\n\
- labels: pick 0-3 from the available list. Empty array if none fit.\n\
\n\
Keep it simple. One JSON object. No extra text.";

pub fn build_todo_messages(transcript: &str, label_names: &[String]) -> Vec<ChatMessage> {
    let today = chrono::Local::now().format("%Y-%m-%d %A").to_string();
    let labels_str = if label_names.is_empty() {
        "none".to_string()
    } else {
        label_names.join(", ")
    };
    let system = format!(
        "Today: {today}\nLabels: [{labels_str}]\n\n{SYSTEM_PROMPT}"
    );
    vec![
        ChatMessage {
            role: "system".into(),
            content: system,
        },
        ChatMessage {
            role: "user".into(),
            content: transcript.to_string(),
        },
    ]
}

/// Build a retry prompt that includes the failed output so the model can self-correct.
pub fn build_retry_messages(
    transcript: &str,
    label_names: &[String],
    failed_json: &str,
) -> Vec<ChatMessage> {
    let mut msgs = build_todo_messages(transcript, label_names);
    msgs.push(ChatMessage {
        role: "assistant".into(),
        content: failed_json.to_string(),
    });
    msgs.push(ChatMessage {
        role: "user".into(),
        content: "Invalid. Return ONLY a raw JSON object. No markdown. No code fences. Fix it."
            .to_string(),
    });
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_todo_messages_has_system_then_user() {
        let labels = vec!["Work".into(), "Personal".into()];
        let msgs = build_todo_messages("Alice: do the thing", &labels);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("Alice: do the thing"));
        assert!(msgs[0].content.contains("Work, Personal"));
    }

    #[test]
    fn system_prompt_demands_json() {
        assert!(SYSTEM_PROMPT.contains("ONLY a single JSON"));
    }

    #[test]
    fn empty_labels_shows_none() {
        let msgs = build_todo_messages("test", &[]);
        assert!(msgs[0].content.contains("[none]"));
    }
}
