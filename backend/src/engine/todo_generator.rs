use sqlx::PgPool;
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::engine::llm_client::LlmClient;
use crate::repository::{speaker as speaker_repo, todo as todo_repo, transcript};
use crate::domain::types::{NewTodo, TodoPriority, Transcript};

/// Maximum transcript length before truncation (in characters).
/// gpt-4o-mini has 128K context, but we cap to control cost.
pub const MAX_TRANSCRIPT_CHARS: usize = 50000; // ~12-15K tokens

pub async fn generate_session_todos(
    pool: &PgPool,
    llm_client: &LlmClient,
    session_id: Uuid,
    tenant_id: Uuid,
) -> Result<(), anyhow::Error> {
    info!(?session_id, "Generating todos for session");

    // 1. Fetch transcripts (only final ones, ordered by time)
    if todo_repo::has_todos(pool, session_id).await? {
        info!(?session_id, "Todos already exist for session, skipping regeneration");
        return Ok(());
    }

    let transcripts = transcript::get_final_transcripts_for_session(pool, session_id).await?;
    if transcripts.is_empty() {
        info!(?session_id, "No transcripts found, skipping todo generation");
        return Ok(());
    }

    // 2. Build transcript string for the LLM
    let transcript_text = build_transcript_string(&transcripts);
    info!(chars = transcript_text.len(), "Built transcript string");

    // 3. Truncate if needed
    let transcript_text = truncate_transcript(&transcript_text);

    // 4. Call LLM
    let llm_items = match llm_client.generate_todos(transcript_text).await {
        Ok(items) => items,
        Err(e) => {
            error!(error = %e, "LLM failed for todo generation");
            return Err(e.into());
        }
    };

    if llm_items.is_empty() {
        info!(?session_id, "LLM returned no action items");
        return Ok(());
    }

    // 5. Convert to NewTodo, resolve speaker names
    let mut new_todos = Vec::new();
    for item in &llm_items {
        let speaker_id = if let Some(ref name) = item.speaker_name {
            match resolve_speaker_id(pool, tenant_id, name).await {
                Ok(id) => id,
                Err(e) => {
                    warn!(speaker_name = name, error = %e, "Failed to resolve speaker");
                    None
                }
            }
        } else {
            None
        };

        new_todos.push(NewTodo {
            session_id,
            speaker_id,
            assigned_to: item.assigned_to.clone(),
            description: item.description.clone(),
            priority: item
                .priority
                .as_deref()
                .and_then(TodoPriority::from_llm_label),
        });
    }

    // 6. Batch insert
    let inserted = todo_repo::create_todos(pool, &new_todos).await?;
    info!(count = inserted.len(), "Inserted todos into database");

    Ok(())
}

/// Build a human-readable transcript string for the LLM.
/// Transcript has no speaker_id field, so we use [Unknown] as label.
/// The LLM will infer speaker assignments from context in the text.
pub fn build_transcript_string(transcripts: &[Transcript]) -> String {
    transcripts
        .iter()
        .map(|t| format!("[Unknown]: {}", t.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Truncate transcript at the last "\n[" boundary if it exceeds MAX_TRANSCRIPT_CHARS.
pub fn truncate_transcript(text: &str) -> &str {
    if text.len() <= MAX_TRANSCRIPT_CHARS {
        return text;
    }

    // Find the last "\n[" within the limit and truncate there
    let truncated = &text[..MAX_TRANSCRIPT_CHARS];
    if let Some(pos) = truncated.rfind("\n[") {
        return &text[..pos];
    }

    // Fall back: hard cut
    &text[..MAX_TRANSCRIPT_CHARS]
}

/// Resolve speaker name to UUID via case-insensitive display_name match.
async fn resolve_speaker_id(
    pool: &PgPool,
    tenant_id: Uuid,
    speaker_name: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let speakers = speaker_repo::list_speakers(pool, tenant_id).await?;
    Ok(speakers
        .iter()
        .find(|s| s.display_name.eq_ignore_ascii_case(speaker_name))
        .map(|s| s.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_transcript_short_enough() {
        let text = "[Alice]: Hello\n[Bob]: Hi";
        let result = truncate_transcript(text);
        assert_eq!(result, text);
    }

    #[test]
    fn test_truncate_at_boundary() {
        let mut text = String::new();
        for i in 0..6000 {
            text.push_str(&format!("[Speaker_{i}] This is some content.\n"));
        }
        let result = truncate_transcript(&text);
        assert!(result.len() <= MAX_TRANSCRIPT_CHARS);
        // Should truncate at a line boundary — the last line may be partial
        // but should not end mid-word (should be clean cut at \n[ boundary)
        assert!(!result.ends_with("\n["));
    }

    #[test]
    fn test_build_transcript_empty() {
        let transcripts: Vec<Transcript> = vec![];
        let result = build_transcript_string(&transcripts);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_transcript_single_item() {
        use chrono::Utc;
        use uuid::Uuid;

        let t = Transcript {
            id: Uuid::nil(),
            session_id: Uuid::nil(),
            segment_id: None,
            start_ms: 0,
            end_ms: 1000,
            text: "Hello world".to_string(),
            is_final: true,
            backend_type: "local".to_string(),
            created_at: Utc::now(),
        };
        let result = build_transcript_string(&[t]);
        assert_eq!(result, "[Unknown]: Hello world");
    }
}
