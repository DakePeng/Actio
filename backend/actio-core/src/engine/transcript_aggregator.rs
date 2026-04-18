use tracing::info;
use uuid::Uuid;

use crate::repository::{speaker, transcript};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AggregatedTranscript {
    pub id: String,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker_id: Option<Uuid>,
    pub is_final: bool,
}

pub struct TranscriptAggregator {
    pool: sqlx::SqlitePool,
    events: tokio::sync::broadcast::Sender<AggregatedTranscript>,
}

impl TranscriptAggregator {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        let (events, _) = tokio::sync::broadcast::channel(256);
        Self { pool, events }
    }

    /// Subscribe to transcript events for WebSocket push.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AggregatedTranscript> {
        self.events.subscribe()
    }

    /// Shared handle to the underlying pool. Used by out-of-band writers like
    /// the per-segment speaker-identification hook so they don't have to be
    /// threaded a separate clone of the pool.
    pub fn pool(&self) -> sqlx::SqlitePool {
        self.pool.clone()
    }

    /// Number of currently-attached broadcast receivers. Used by the
    /// pipeline supervisor to decide whether to keep the always-on
    /// recognizer warm or hibernate it to free RAM.
    pub fn receiver_count(&self) -> usize {
        self.events.receiver_count()
    }

    fn publish(&self, t: &AggregatedTranscript) {
        // Ignore send errors — no active subscribers is fine
        let _ = self.events.send(t.clone());
    }

    /// Broadcast a partial transcript to WS subscribers without persisting to DB.
    pub fn broadcast_partial(&self, text: &str, start_ms: i64, end_ms: i64) {
        let t = AggregatedTranscript {
            id: String::new(),
            text: text.to_string(),
            start_ms,
            end_ms,
            speaker_id: None,
            is_final: false,
        };
        self.publish(&t);
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
            &self.pool, session_id, text, start_ms, end_ms, false, segment_id,
        )
        .await?;

        let result = AggregatedTranscript {
            id: t.id,
            text: text.to_string(),
            start_ms: t.start_ms,
            end_ms: t.end_ms,
            speaker_id: None,
            is_final: false,
        };
        self.publish(&result);
        Ok(result)
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
            &self.pool, session_id, text, start_ms, end_ms, true, segment_id,
        )
        .await?;

        let result = AggregatedTranscript {
            id: t.id,
            text: text.to_string(),
            start_ms: t.start_ms,
            end_ms: t.end_ms,
            speaker_id: None,
            is_final: true,
        };
        self.publish(&result);
        Ok(result)
    }

    pub async fn finalize(
        &self,
        transcript_id: Uuid,
        final_text: &str,
        speaker_id: Option<Uuid>,
    ) -> Result<AggregatedTranscript, sqlx::Error> {
        let updated =
            transcript::finalize_transcript(&self.pool, transcript_id, final_text).await?;

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

        let result = AggregatedTranscript {
            id: updated.id,
            text: display_text,
            start_ms: updated.start_ms,
            end_ms: updated.end_ms,
            speaker_id,
            is_final: true,
        };
        self.publish(&result);
        Ok(result)
    }

    /// Backfill speaker tag on a previously stored transcript, then re-finalize it.
    pub async fn backfill_speaker(
        &self,
        transcript_id: Uuid,
        speaker_id: Uuid,
    ) -> Result<AggregatedTranscript, sqlx::Error> {
        let row: (String,) = sqlx::query_as("SELECT text FROM transcripts WHERE id = ?1")
            .bind(transcript_id.to_string())
            .fetch_one(&self.pool)
            .await?;

        self.finalize(transcript_id, &row.0, Some(speaker_id)).await
    }
}
