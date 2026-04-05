use crate::domain::types::Transcript;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_transcript(
    pool: &PgPool,
    session_id: Uuid,
    text: &str,
    start_ms: i64,
    end_ms: i64,
    is_final: bool,
    segment_id: Option<Uuid>,
) -> Result<Transcript, sqlx::Error> {
    sqlx::query_as::<_, Transcript>(
        "INSERT INTO transcripts (session_id, segment_id, start_ms, end_ms, text, is_final) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *"
    )
    .bind(session_id)
    .bind(segment_id)
    .bind(start_ms)
    .bind(end_ms)
    .bind(text)
    .bind(is_final)
    .fetch_one(pool)
    .await
}

pub async fn finalize_transcript(
    pool: &PgPool,
    id: Uuid,
    text: &str,
) -> Result<Transcript, sqlx::Error> {
    sqlx::query_as::<_, Transcript>(
        "UPDATE transcripts SET text = $1, is_final = true WHERE id = $2 RETURNING *"
    )
    .bind(text)
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn get_transcripts_for_session(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Vec<Transcript>, sqlx::Error> {
    sqlx::query_as::<_, Transcript>(
        "SELECT * FROM transcripts WHERE session_id = $1 ORDER BY start_ms"
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
}

pub async fn get_final_transcripts_for_session(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Vec<Transcript>, sqlx::Error> {
    sqlx::query_as::<_, Transcript>(
        "SELECT * FROM transcripts WHERE session_id = $1 AND is_final = true ORDER BY start_ms"
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
}
