use tracing::info;
use uuid::Uuid;

use crate::repository::{speaker, transcript};

#[derive(Debug, Clone)]
pub struct AggregatedTranscript {
    pub id: Uuid,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_id: Option<Uuid>,
    pub is_final: bool,
}

pub struct TranscriptAggregator {
    pool: sqlx::PgPool,
}

impl TranscriptAggregator {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    pub async fn add_partial(
        &self,
        session_id: Uuid,
        text: &str,
        start_ms: i64,
        end_ms: i64,
        segment_id: Option<Uuid>,
    ) -> Result<AggregatedTranscript, sqlx::Error> {
        let t = transcript::create_transcript(
            &self.pool,
            session_id,
            text,
            start_ms,
            end_ms,
            false,
            segment_id,
        )
        .await?;

        Ok(AggregatedTranscript {
            id: t.id,
            text: format!("[UNKNOWN] {}", text),
            start_ms: t.start_ms,
            end_ms: t.end_ms,
            speaker_id: None,
            is_final: false,
        })
    }

    pub async fn add_final(
        &self,
        session_id: Uuid,
        text: &str,
        start_ms: i64,
        end_ms: i64,
        segment_id: Option<Uuid>,
    ) -> Result<AggregatedTranscript, sqlx::Error> {
        let t = transcript::create_transcript(
            &self.pool,
            session_id,
            text,
            start_ms,
            end_ms,
            true,
            segment_id,
        )
        .await?;

        Ok(AggregatedTranscript {
            id: t.id,
            text: format!("[UNKNOWN] {}", text),
            start_ms: t.start_ms,
            end_ms: t.end_ms,
            speaker_id: None,
            is_final: true,
        })
    }

    pub async fn finalize(
        &self,
        transcript_id: Uuid,
        final_text: &str,
        speaker_id: Option<Uuid>,
    ) -> Result<AggregatedTranscript, sqlx::Error> {
        let updated = transcript::finalize_transcript(&self.pool, transcript_id, final_text).await?;

        let display_text = match &speaker_id {
            Some(sid) => {
                if let Ok(s) = speaker::get_speaker(&self.pool, *sid).await {
                    format!("[{}] {}", s.display_name, final_text)
                } else {
                    format!("[UNKNOWN] {}", final_text)
                }
            }
            None => format!("[UNKNOWN] {}", final_text),
        };

        info!(%transcript_id, ?speaker_id, "Transcript finalized");

        Ok(AggregatedTranscript {
            id: updated.id,
            text: display_text,
            start_ms: updated.start_ms,
            end_ms: updated.end_ms,
            speaker_id,
            is_final: true,
        })
    }
}
