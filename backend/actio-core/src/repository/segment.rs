use sqlx::SqlitePool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UnknownSegmentRow {
    pub id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub embedding: Option<Vec<u8>>,
    pub embedding_dim: Option<i64>,
}

pub async fn list_unknown_segments(
    pool: &SqlitePool,
    session_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<UnknownSegmentRow>, sqlx::Error> {
    if let Some(sid) = session_id {
        sqlx::query_as::<_, UnknownSegmentRow>(
            "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
             FROM audio_segments \
             WHERE session_id = ?1 AND speaker_id IS NULL \
             ORDER BY start_ms LIMIT ?2",
        )
        .bind(sid.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, UnknownSegmentRow>(
            "SELECT id, session_id, start_ms, end_ms, embedding, embedding_dim \
             FROM audio_segments WHERE speaker_id IS NULL \
             ORDER BY start_ms DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

/// Row shape for Phase-B clustering: only segments with a retained WAV on
/// disk AND a populated embedding can be surfaced as voiceprint candidates.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RetainedCandidateRow {
    pub id: String,
    pub session_id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub audio_ref: String,
    pub embedding: Vec<u8>,
}

pub async fn list_retained_candidates(
    pool: &SqlitePool,
    session_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<RetainedCandidateRow>, sqlx::Error> {
    if let Some(sid) = session_id {
        sqlx::query_as::<_, RetainedCandidateRow>(
            "SELECT id, session_id, start_ms, end_ms, audio_ref, embedding \
             FROM audio_segments \
             WHERE session_id = ?1 AND speaker_id IS NULL \
               AND audio_ref IS NOT NULL AND embedding IS NOT NULL \
               AND dismissed_at IS NULL \
             ORDER BY start_ms DESC LIMIT ?2",
        )
        .bind(sid.to_string())
        .bind(limit)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, RetainedCandidateRow>(
            "SELECT id, session_id, start_ms, end_ms, audio_ref, embedding \
             FROM audio_segments \
             WHERE speaker_id IS NULL \
               AND audio_ref IS NOT NULL AND embedding IS NOT NULL \
               AND dismissed_at IS NULL \
             ORDER BY start_ms DESC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(pool)
        .await
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SegmentByIdRow {
    pub id: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub embedding: Option<Vec<u8>>,
    pub audio_ref: Option<String>,
}

/// Fetch a set of segments by id. Used by the candidate-confirm and
/// candidate-dismiss handlers to snapshot audio_refs before nulling them.
pub async fn fetch_segments_by_ids(
    pool: &SqlitePool,
    ids: &[String],
) -> Result<Vec<SegmentByIdRow>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT id, start_ms, end_ms, embedding, audio_ref \
         FROM audio_segments WHERE id IN ({})",
        placeholders.join(","),
    );
    let mut q = sqlx::query_as::<_, SegmentByIdRow>(&sql);
    for id in ids {
        q = q.bind(id);
    }
    q.fetch_all(pool).await
}

/// Mark a set of segments as dismissed — they no longer surface as
/// voiceprint candidates. The caller is responsible for deleting the
/// retained WAV files from disk.
pub async fn mark_segments_dismissed(
    pool: &SqlitePool,
    ids: &[String],
) -> Result<u64, sqlx::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "UPDATE audio_segments \
         SET dismissed_at = datetime('now'), audio_ref = NULL \
         WHERE id IN ({})",
        placeholders.join(","),
    );
    let mut q = sqlx::query(&sql);
    for id in ids {
        q = q.bind(id);
    }
    let r = q.execute(pool).await?;
    Ok(r.rows_affected())
}

/// Assign a set of segments to a speaker in one shot. Clears `audio_ref`
/// so the retained WAVs can be deleted and the rows stop appearing in
/// candidate queries.
pub async fn assign_segments_to_speaker(
    pool: &SqlitePool,
    ids: &[String],
    speaker_id: Uuid,
) -> Result<u64, sqlx::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders: Vec<String> = (2..=(ids.len() + 1)).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "UPDATE audio_segments \
         SET speaker_id = ?1, audio_ref = NULL \
         WHERE id IN ({})",
        placeholders.join(","),
    );
    let mut q = sqlx::query(&sql).bind(speaker_id.to_string());
    for id in ids {
        q = q.bind(id);
    }
    let r = q.execute(pool).await?;
    Ok(r.rows_affected())
}

/// Label-only assignment: sets `audio_segments.speaker_id` so past
/// transcripts render the right name. Does NOT promote the segment's
/// embedding to a voiceprint — voiceprints come exclusively from curated
/// enrollment clips via `POST /speakers/{id}/enroll`.
pub async fn assign_speaker(
    pool: &SqlitePool,
    segment_id: Uuid,
    speaker_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("UPDATE audio_segments SET speaker_id = ?1 WHERE id = ?2")
        .bind(speaker_id.to_string())
        .bind(segment_id.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn unassign_speaker(pool: &SqlitePool, segment_id: Uuid) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("UPDATE audio_segments SET speaker_id = NULL WHERE id = ?1")
        .bind(segment_id.to_string())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Insert an audio_segments row, optionally attaching a pre-computed embedding
/// and speaker identification result. Used by the live inference pipeline as
/// each VAD-detected segment completes.
///
/// The `id` argument MUST be the same UUID the VAD attached to the
/// `SpeechSegment` and that the offline-ASR consumer carries through to
/// `transcripts.segment_id`. Generating a fresh UUID here would orphan
/// transcripts (FK constraint failure) since they reference the VAD-side
/// id, not whatever this function picked.
pub async fn insert_segment(
    pool: &SqlitePool,
    id: Uuid,
    session_id: Uuid,
    start_ms: i64,
    end_ms: i64,
    speaker_id: Option<Uuid>,
    speaker_score: Option<f64>,
    embedding: Option<&[f32]>,
    audio_ref: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let id_str = id.to_string();
    let (blob, dim) = match embedding {
        Some(e) => (
            Some(bytemuck::cast_slice::<f32, u8>(e).to_vec()),
            Some(e.len() as i64),
        ),
        None => (None, None),
    };
    sqlx::query(
        "INSERT INTO audio_segments \
           (id, session_id, start_ms, end_ms, speaker_id, speaker_score, embedding, embedding_dim, audio_ref) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(&id_str)
    .bind(session_id.to_string())
    .bind(start_ms)
    .bind(end_ms)
    .bind(speaker_id.map(|u| u.to_string()))
    .bind(speaker_score)
    .bind(blob)
    .bind(dim)
    .bind(audio_ref)
    .execute(pool)
    .await?;
    Ok(id)
}

/// Update the embedding + dimension for an existing audio_segments row.
/// Used by the batch processor after the embedder produces vectors for
/// each segment. Idempotent — re-running batch processing for a clip just
/// overwrites with the (identical) vector.
pub async fn set_embedding(
    pool: &SqlitePool,
    id: Uuid,
    embedding: &[f32],
    dim: i64,
) -> Result<(), sqlx::Error> {
    let blob = bytemuck::cast_slice::<f32, u8>(embedding).to_vec();
    sqlx::query(
        "UPDATE audio_segments \
         SET embedding = ?2, embedding_dim = ?3 \
         WHERE id = ?1",
    )
    .bind(id.to_string())
    .bind(blob)
    .bind(dim)
    .execute(pool)
    .await?;
    Ok(())
}

/// Set both the speaker assignment and the clip-local cluster index for
/// a segment. Called once per cluster member after the centroid match
/// resolves either to an existing speaker or a fresh provisional row.
pub async fn assign_speaker_and_local_idx(
    pool: &SqlitePool,
    seg_id: Uuid,
    speaker_id: Uuid,
    local_idx: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE audio_segments \
         SET speaker_id = ?2, clip_local_speaker_idx = ?3 \
         WHERE id = ?1",
    )
    .bind(seg_id.to_string())
    .bind(speaker_id.to_string())
    .bind(local_idx)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClipSegmentRow {
    pub id: String,
    pub session_id: String,
    pub clip_id: Option<String>,
    pub speaker_id: Option<String>,
    pub start_ms: i64,
    pub end_ms: i64,
    pub clip_local_speaker_idx: Option<i64>,
}

/// All audio_segments rows tied to a clip, ordered by time. Backs the
/// archive UI's "show clip transcripts with speaker labels" view.
pub async fn list_for_clip(
    pool: &SqlitePool,
    clip_id: Uuid,
) -> Result<Vec<ClipSegmentRow>, sqlx::Error> {
    sqlx::query_as::<_, ClipSegmentRow>(
        "SELECT id, session_id, clip_id, speaker_id, start_ms, end_ms, clip_local_speaker_idx \
         FROM audio_segments \
         WHERE clip_id = ?1 \
         ORDER BY start_ms",
    )
    .bind(clip_id.to_string())
    .fetch_all(pool)
    .await
}

/// Insert (or no-op-update clip_id on) an audio_segments row tied to a
/// batch-processed clip. Used by the batch processor before transcripts
/// land for each segment in a clip's manifest. Idempotent — re-running
/// batch processing for a clip does not duplicate rows.
pub async fn upsert_segment_for_clip(
    pool: &SqlitePool,
    id: Uuid,
    session_id: Uuid,
    clip_id: Uuid,
    start_ms: i64,
    end_ms: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audio_segments (id, session_id, clip_id, start_ms, end_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5) \
         ON CONFLICT(id) DO UPDATE SET clip_id = excluded.clip_id",
    )
    .bind(id.to_string())
    .bind(session_id.to_string())
    .bind(clip_id.to_string())
    .bind(start_ms)
    .bind(end_ms)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::speaker::create_speaker;

    use crate::testing::fresh_pool;

    async fn insert_session(pool: &SqlitePool) -> String {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO audio_sessions (id) VALUES (?1)")
            .bind(&id)
            .execute(pool)
            .await
            .unwrap();
        id
    }

    async fn insert_unknown_segment(pool: &SqlitePool, session_id: &str, start: i64) -> String {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audio_segments \
               (id, session_id, start_ms, end_ms, embedding, embedding_dim) \
             VALUES (?1, ?2, ?3, ?4, ?5, 512)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(start)
        .bind(start + 1000)
        .bind(bytemuck::cast_slice::<f32, u8>(&vec![0.5f32; 512]))
        .execute(pool)
        .await
        .unwrap();
        id
    }

    #[tokio::test]
    async fn lists_unknowns_per_session() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        insert_unknown_segment(&pool, &sid, 0).await;
        insert_unknown_segment(&pool, &sid, 1000).await;
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[tokio::test]
    async fn assign_sets_speaker_id_and_removes_from_unknowns() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        let seg_id = insert_unknown_segment(&pool, &sid, 0).await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();

        let ok = assign_speaker(
            &pool,
            Uuid::parse_str(&seg_id).unwrap(),
            Uuid::parse_str(&alice.id).unwrap(),
        )
        .await
        .unwrap();
        assert!(ok);

        // Unknown list for the session is now empty.
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10)
            .await
            .unwrap();
        assert!(rows.is_empty());

        // Critical: assign must NOT promote the segment's embedding to a voiceprint.
        let voiceprint_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM speaker_embeddings WHERE speaker_id = ?1")
                .bind(&alice.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            voiceprint_count.0, 0,
            "retroactive tagging must not create speaker_embeddings rows"
        );
    }

    #[tokio::test]
    async fn insert_segment_uses_caller_provided_id_so_transcripts_can_fk_to_it() {
        // Regression: insert_segment used to generate its own Uuid::new_v4() internally,
        // which orphaned every transcript whose segment_id came from VAD. The FK on
        // transcripts.segment_id → audio_segments.id then failed at insert time and the
        // always-listening action extractor saw zero usable transcript rows.
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        let session_uuid = Uuid::parse_str(&sid).unwrap();
        let segment_id = Uuid::new_v4();

        let returned = insert_segment(
            &pool,
            segment_id,
            session_uuid,
            0,
            1000,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(
            returned, segment_id,
            "insert_segment must echo the caller's id"
        );

        // Transcript referencing the same segment_id must FK-resolve.
        let transcript_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO transcripts (id, session_id, segment_id, start_ms, end_ms, text, is_final) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
        )
        .bind(&transcript_id)
        .bind(&sid)
        .bind(segment_id.to_string())
        .bind(0i64)
        .bind(1000i64)
        .bind("hello world")
        .execute(&pool)
        .await
        .expect("FK to audio_segments.id must hold when ids match");
    }

    #[tokio::test]
    async fn unassign_nulls_speaker_id() {
        let pool = fresh_pool().await;
        let sid = insert_session(&pool).await;
        let seg_id = insert_unknown_segment(&pool, &sid, 0).await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        assign_speaker(
            &pool,
            Uuid::parse_str(&seg_id).unwrap(),
            Uuid::parse_str(&alice.id).unwrap(),
        )
        .await
        .unwrap();

        assert!(unassign_speaker(&pool, Uuid::parse_str(&seg_id).unwrap())
            .await
            .unwrap());

        // Segment is back in the unknown list.
        let rows = list_unknown_segments(&pool, Some(Uuid::parse_str(&sid).unwrap()), 10)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
    }
}
