use sqlx::SqlitePool;
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::engine::llm_router::LlmRouter;
use crate::repository::{speaker as speaker_repo, reminder as reminder_repo, transcript};
use crate::domain::types::{NewReminder, Transcript};

/// Maximum transcript length before truncation (in characters).
/// gpt-4o-mini has 128K context, but we cap to control cost.
pub const MAX_TRANSCRIPT_CHARS: usize = 50000; // ~12-15K tokens

pub async fn generate_session_todos(
    pool: &SqlitePool,
    router: &LlmRouter,
    session_id: Uuid,
    tenant_id: Uuid,
) -> Result<(), anyhow::Error> {
    info!(?session_id, "Generating reminders for session");

    if reminder_repo::has_reminders(pool, session_id).await? {
        info!(?session_id, "Reminders already exist for session, skipping");
        return Ok(());
    }

    let transcripts = transcript::get_final_transcripts_for_session(pool, session_id).await?;
    if transcripts.is_empty() {
        info!(?session_id, "No transcripts found, skipping reminder generation");
        return Ok(());
    }

    let transcript_text = build_transcript_string(&transcripts);
    info!(chars = transcript_text.len(), "Built transcript string");
    let transcript_text = truncate_transcript(&transcript_text);

    let llm_items = match router.generate_todos(transcript_text, &[]).await {
        Ok(items) => items,
        Err(e) => {
            error!(error = %e, "LLM router failed for reminder generation");
            return Err(e.into());
        }
    };

    if llm_items.is_empty() {
        info!(?session_id, "LLM returned no action items");
        return Ok(());
    }

    let mut new_reminders = Vec::new();
    for item in &llm_items {
        new_reminders.push(NewReminder {
            session_id: Some(session_id),
            tenant_id,
            speaker_id: None,
            assigned_to: None,
            title: None,
            description: item.description.clone(),
            priority: item.priority.clone(),
            due_time: None,
            transcript_excerpt: None,
            context: None,
            source_time: None,
        });
    }

    let inserted = reminder_repo::create_reminders_batch(pool, &new_reminders).await?;
    info!(count = inserted.len(), "Inserted reminders into database");

    Ok(())
}

/// Build a human-readable transcript string for the LLM.
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
    let truncated = &text[..MAX_TRANSCRIPT_CHARS];
    if let Some(pos) = truncated.rfind("\n[") {
        return &text[..pos];
    }
    &text[..MAX_TRANSCRIPT_CHARS]
}

/// Resolve speaker name to UUID via case-insensitive display_name match.
async fn resolve_speaker_id(
    pool: &SqlitePool,
    tenant_id: Uuid,
    speaker_name: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    let speakers = speaker_repo::list_speakers(pool, tenant_id).await?;
    Ok(speakers
        .iter()
        .find(|s| s.display_name.eq_ignore_ascii_case(speaker_name))
        .and_then(|s| s.id.parse().ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_transcript_short_enough() {
        let text = "[Alice]: Hello\n[Bob]: Hi";
        assert_eq!(truncate_transcript(text), text);
    }

    #[test]
    fn test_truncate_at_boundary() {
        let mut text = String::new();
        for i in 0..6000 {
            text.push_str(&format!("[Speaker_{i}] This is some content.\n"));
        }
        let result = truncate_transcript(&text);
        assert!(result.len() <= MAX_TRANSCRIPT_CHARS);
        assert!(!result.ends_with("\n["));
    }

    #[test]
    fn test_build_transcript_empty() {
        let transcripts: Vec<Transcript> = vec![];
        assert!(build_transcript_string(&transcripts).is_empty());
    }

    #[test]
    fn test_build_transcript_single_item() {
        use chrono::Utc;
        let t = Transcript {
            id: "00000000-0000-0000-0000-000000000000".to_string(),
            session_id: "00000000-0000-0000-0000-000000000000".to_string(),
            segment_id: None,
            start_ms: 0,
            end_ms: 1000,
            text: "Hello world".to_string(),
            is_final: true,
            backend_type: "local".to_string(),
            created_at: Utc::now(),
        };
        assert_eq!(build_transcript_string(&[t]), "[Unknown]: Hello world");
    }
}
