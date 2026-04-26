use crate::domain::types::Speaker;
use sqlx::SqlitePool;
use uuid::Uuid;

pub async fn create_speaker(
    pool: &SqlitePool,
    display_name: &str,
    color: &str,
    tenant_id: Uuid,
) -> Result<Speaker, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    sqlx::query_as::<_, Speaker>(
        "INSERT INTO speakers (id, display_name, color, tenant_id) \
         VALUES (?1, ?2, ?3, ?4) RETURNING *",
    )
    .bind(&id)
    .bind(display_name)
    .bind(color)
    .bind(tenant_id.to_string())
    .fetch_one(pool)
    .await
}

pub async fn get_speaker(pool: &SqlitePool, id: Uuid) -> Result<Speaker, sqlx::Error> {
    sqlx::query_as::<_, Speaker>("SELECT * FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .fetch_one(pool)
        .await
}

pub async fn list_speakers(
    pool: &SqlitePool,
    tenant_id: Uuid,
) -> Result<Vec<Speaker>, sqlx::Error> {
    sqlx::query_as::<_, Speaker>(
        "SELECT * FROM speakers WHERE tenant_id = ?1 AND status = 'active' \
         ORDER BY created_at DESC",
    )
    .bind(tenant_id.to_string())
    .fetch_all(pool)
    .await
}

pub async fn update_speaker(
    pool: &SqlitePool,
    id: Uuid,
    display_name: Option<&str>,
    color: Option<&str>,
) -> Result<Option<Speaker>, sqlx::Error> {
    // COALESCE lets us pass NULL for "keep existing" per field.
    sqlx::query_as::<_, Speaker>(
        "UPDATE speakers \
         SET display_name = COALESCE(?1, display_name), \
             color = COALESCE(?2, color) \
         WHERE id = ?3 RETURNING *",
    )
    .bind(display_name)
    .bind(color)
    .bind(id.to_string())
    .fetch_optional(pool)
    .await
}

/// Single-transaction delete with segment FK cleanup.
pub async fn delete_speaker_with_segment_cleanup(
    pool: &SqlitePool,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE audio_segments SET speaker_id = NULL WHERE speaker_id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query("DELETE FROM speakers WHERE id = ?1")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    // On any error above, `tx` drops without commit(), implicitly rolling back both writes.
    tx.commit().await?;
    Ok(result.rows_affected() > 0)
}

// ── Provisional speakers (batch clip processing) ─────────────────────────

/// Insert a new provisional speaker. The batch processor calls this when a
/// per-clip cluster centroid does not match any existing speaker (enrolled
/// or provisional). The display_name is auto-generated and meant to be
/// renamed by the user via the Candidate Speakers panel.
pub async fn insert_provisional(
    pool: &SqlitePool,
    id: Uuid,
    tenant_id: Uuid,
    display_name: &str,
    color: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO speakers \
           (id, tenant_id, display_name, color, status, kind, provisional_last_matched_at) \
         VALUES (?1, ?2, ?3, ?4, 'active', 'provisional', \
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
    )
    .bind(id.to_string())
    .bind(tenant_id.to_string())
    .bind(display_name)
    .bind(color)
    .execute(pool)
    .await?;
    Ok(())
}

/// Promote a provisional speaker to enrolled, optionally renaming. After
/// promotion the row is just like any other enrolled speaker — it shows
/// up in `list_speakers`, `find_match_by_centroid`'s enrolled pool, etc.
/// `provisional_last_matched_at` is cleared so the GC sweep no longer
/// touches it. Returns true if the row existed and was promoted.
pub async fn promote_provisional(
    pool: &SqlitePool,
    id: Uuid,
    new_display_name: Option<&str>,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        "UPDATE speakers \
         SET kind = 'enrolled', \
             display_name = COALESCE(?2, display_name), \
             provisional_last_matched_at = NULL \
         WHERE id = ?1 AND kind = 'provisional'",
    )
    .bind(id.to_string())
    .bind(new_display_name)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

/// Hard-delete a provisional speaker. The user explicitly told us this
/// row isn't a real person worth tracking. Attached audio_segments rows
/// have their speaker_id set to NULL via the existing FK. Returns true if
/// a row was actually removed.
pub async fn dismiss_provisional(pool: &SqlitePool, id: Uuid) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM speakers WHERE id = ?1 AND kind = 'provisional'")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Hard-delete provisional speakers whose last match is older than
/// `older_than_days`. Their attached audio_segments rows have their
/// `speaker_id` set to NULL via the existing ON DELETE SET NULL FK.
/// Returns the number of rows deleted. NULL `provisional_last_matched_at`
/// (shouldn't happen for any row inserted by `insert_provisional`, but
/// allowed by the schema) is treated as eligible for GC.
pub async fn gc_stale_provisionals(
    pool: &SqlitePool,
    older_than_days: i64,
) -> Result<u64, sqlx::Error> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(older_than_days);
    let cutoff_str = cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let res = sqlx::query(
        "DELETE FROM speakers \
         WHERE kind = 'provisional' \
           AND (provisional_last_matched_at IS NULL \
                OR provisional_last_matched_at < ?1)",
    )
    .bind(cutoff_str)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

/// Bump `provisional_last_matched_at` to now. Called when a later clip's
/// cluster centroid matches an existing provisional row, so the GC sweep
/// in Plan Task 14 doesn't reap an actively-used provisional speaker.
pub async fn touch_provisional_match(pool: &SqlitePool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE speakers \
         SET provisional_last_matched_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') \
         WHERE id = ?1 AND kind = 'provisional'",
    )
    .bind(id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProvisionalSpeakerRow {
    pub id: String,
    pub tenant_id: String,
    pub display_name: String,
    pub color: String,
    pub provisional_last_matched_at: Option<String>,
}

/// List all currently-active provisional speakers, newest match first.
/// Backs the Candidate Speakers panel (Plan Task 15).
pub async fn list_provisional(
    pool: &SqlitePool,
) -> Result<Vec<ProvisionalSpeakerRow>, sqlx::Error> {
    sqlx::query_as::<_, ProvisionalSpeakerRow>(
        "SELECT id, tenant_id, display_name, color, provisional_last_matched_at \
         FROM speakers \
         WHERE kind = 'provisional' AND status = 'active' \
         ORDER BY provisional_last_matched_at DESC",
    )
    .fetch_all(pool)
    .await
}

/// Match a cluster centroid against all known speakers in `tenant_id`,
/// returning the best-matching speaker_id whose mean embedding clears
/// `confirm_threshold` cosine similarity. Returns None if nothing matches.
///
/// Two pools are joined:
///   * enrolled speakers — averaged across their `speaker_embeddings` rows
///   * provisional speakers — averaged across their attached
///     `audio_segments.embedding` BLOBs (set by the batch processor when
///     it created/extended the cluster).
///
/// Candidates with a different `embedding_dimension` than the query
/// centroid are silently skipped — see CLAUDE.md on per-model dimensions.
pub async fn find_match_by_centroid(
    pool: &SqlitePool,
    centroid: &[f32],
    dim: i64,
    tenant_id: Uuid,
    confirm_threshold: f32,
) -> Result<Option<Uuid>, sqlx::Error> {
    if centroid.is_empty() {
        return Ok(None);
    }

    // Enrolled — speaker_embeddings table.
    let enrolled: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT e.speaker_id, e.embedding \
         FROM speaker_embeddings e \
         JOIN speakers s ON s.id = e.speaker_id \
         WHERE s.tenant_id = ?1 \
           AND s.status = 'active' \
           AND s.kind = 'enrolled' \
           AND e.embedding_dimension = ?2",
    )
    .bind(tenant_id.to_string())
    .bind(dim)
    .fetch_all(pool)
    .await?;

    // Provisional — audio_segments embeddings tied to provisional speakers.
    let provisional: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT seg.speaker_id, seg.embedding \
         FROM audio_segments seg \
         JOIN speakers s ON s.id = seg.speaker_id \
         WHERE s.tenant_id = ?1 \
           AND s.status = 'active' \
           AND s.kind = 'provisional' \
           AND seg.embedding IS NOT NULL \
           AND seg.embedding_dim = ?2",
    )
    .bind(tenant_id.to_string())
    .bind(dim)
    .fetch_all(pool)
    .await?;

    let candidates: Vec<(String, Vec<u8>)> =
        enrolled.into_iter().chain(provisional.into_iter()).collect();
    if candidates.is_empty() {
        return Ok(None);
    }

    // Group by speaker, mean-pool, normalize, score against centroid.
    let dim_us = dim as usize;
    let mut sums: std::collections::BTreeMap<String, (Vec<f32>, usize)> = Default::default();
    for (speaker_id, blob) in candidates {
        let v: &[f32] = bytemuck::cast_slice(&blob);
        if v.len() != dim_us {
            continue;
        }
        let entry = sums
            .entry(speaker_id)
            .or_insert_with(|| (vec![0.0_f32; dim_us], 0));
        for (i, x) in v.iter().enumerate() {
            entry.0[i] += x;
        }
        entry.1 += 1;
    }

    let q = unit_normalize(centroid);
    let mut best: Option<(String, f32)> = None;
    for (speaker_id, (sum, n)) in sums {
        if n == 0 {
            continue;
        }
        let mean: Vec<f32> = sum.iter().map(|x| x / n as f32).collect();
        let unit = unit_normalize(&mean);
        let sim = cosine_similarity(&q, &unit);
        if best.as_ref().map_or(true, |(_, b)| sim > *b) {
            best = Some((speaker_id, sim));
        }
    }
    Ok(best.and_then(|(id, sim)| {
        if sim >= confirm_threshold {
            Uuid::parse_str(&id).ok()
        } else {
            None
        }
    }))
}

fn unit_normalize(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-8);
    v.iter().map(|x| x / n).collect()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// Atomically flip `is_self=1` on the target speaker, clearing any prior
/// self-flag for the same tenant. Also syncs the speaker's `display_name`
/// into `tenant_profile.display_name` so the extraction prompt's
/// bracketed-tag reference matches the transcript tag.
/// Defense in depth alongside the partial unique index
/// `idx_speakers_one_self_per_tenant`.
pub async fn mark_as_self(pool: &SqlitePool, speaker_id: Uuid) -> sqlx::Result<()> {
    let mut tx = pool.begin().await?;

    let row: (String, String) = sqlx::query_as(
        "SELECT tenant_id, display_name FROM speakers WHERE id = ?1",
    )
    .bind(speaker_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    let (tenant_id, speaker_name) = row;

    sqlx::query(
        "UPDATE speakers SET is_self = 0 WHERE tenant_id = ?1 AND is_self = 1",
    )
    .bind(&tenant_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query("UPDATE speakers SET is_self = 1 WHERE id = ?1")
        .bind(speaker_id.to_string())
        .execute(&mut *tx)
        .await?;

    // Sync the speaker's name into tenant_profile.display_name so the
    // extraction prompt's bracketed-tag reference matches the transcript tag.
    // Preserves existing aliases / bio when the row already exists.
    sqlx::query(
        r#"INSERT INTO tenant_profile (tenant_id, display_name, aliases, bio, updated_at)
           VALUES (?1, ?2, '[]', NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
           ON CONFLICT(tenant_id) DO UPDATE SET
             display_name = excluded.display_name,
             updated_at   = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"#,
    )
    .bind(&tenant_id)
    .bind(&speaker_name)
    .execute(&mut *tx)
    .await?;

    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::db::run_migrations;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn fresh_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn create_speaker_persists_color() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        assert_eq!(s.display_name, "Alice");
        assert_eq!(s.color, "#E57373");
    }

    #[tokio::test]
    async fn update_speaker_applies_patch() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let updated = update_speaker(
            &pool,
            Uuid::parse_str(&s.id).unwrap(),
            Some("Alicia"),
            Some("#64B5F6"),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(updated.display_name, "Alicia");
        assert_eq!(updated.color, "#64B5F6");
    }

    #[tokio::test]
    async fn delete_speaker_clears_segment_refs() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        // Insert a minimal audio_session and audio_segment referencing this speaker.
        let session_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audio_sessions (id, tenant_id, source_type, mode, started_at) \
             VALUES (?1, ?2, 'microphone', 'realtime', datetime('now'))",
        )
        .bind(&session_id)
        .bind(Uuid::nil().to_string())
        .execute(&pool)
        .await
        .unwrap();
        let segment_id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO audio_segments (id, session_id, speaker_id, start_ms, end_ms) \
             VALUES (?1, ?2, ?3, 0, 1000)",
        )
        .bind(&segment_id)
        .bind(&session_id)
        .bind(&s.id)
        .execute(&pool)
        .await
        .unwrap();

        let deleted = delete_speaker_with_segment_cleanup(&pool, Uuid::parse_str(&s.id).unwrap())
            .await
            .unwrap();
        assert!(deleted);

        // Speaker row should be gone.
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM speakers WHERE id = ?1")
            .bind(&s.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);

        // Segment should have speaker_id = NULL.
        let seg_speaker: (Option<String>,) =
            sqlx::query_as("SELECT speaker_id FROM audio_segments WHERE id = ?1")
                .bind(&segment_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(seg_speaker.0.is_none());
    }

    #[tokio::test]
    async fn mark_as_self_clears_prior_self_for_same_tenant() {
        let pool = fresh_pool().await;

        let tenant_id = Uuid::new_v4();
        let alice = Uuid::new_v4();
        let bob = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO speakers (id, tenant_id, display_name) VALUES (?1, ?2, 'Alice')",
        )
        .bind(alice.to_string())
        .bind(tenant_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO speakers (id, tenant_id, display_name) VALUES (?1, ?2, 'Bob')",
        )
        .bind(bob.to_string())
        .bind(tenant_id.to_string())
        .execute(&pool)
        .await
        .unwrap();

        super::mark_as_self(&pool, alice).await.unwrap();
        super::mark_as_self(&pool, bob).await.unwrap();

        let alice_flag: (i64,) = sqlx::query_as("SELECT is_self FROM speakers WHERE id = ?1")
            .bind(alice.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        let bob_flag: (i64,) = sqlx::query_as("SELECT is_self FROM speakers WHERE id = ?1")
            .bind(bob.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(alice_flag.0, 0);
        assert_eq!(bob_flag.0, 1);
    }

    #[tokio::test]
    async fn mark_as_self_syncs_display_name_into_tenant_profile() {
        use crate::repository::db::run_migrations;
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:").await.unwrap();
        sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await.unwrap();
        run_migrations(&pool).await.unwrap();

        let tenant_id = Uuid::new_v4();
        let speaker_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO speakers (id, tenant_id, display_name) VALUES (?1, ?2, 'Tray Mic 1')",
        ).bind(speaker_id.to_string()).bind(tenant_id.to_string())
        .execute(&pool).await.unwrap();

        super::mark_as_self(&pool, speaker_id).await.unwrap();

        let profile_name: (String,) = sqlx::query_as(
            "SELECT display_name FROM tenant_profile WHERE tenant_id = ?1"
        ).bind(tenant_id.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(profile_name.0, "Tray Mic 1");
    }

    #[tokio::test]
    async fn mark_as_self_preserves_existing_aliases_and_bio() {
        use crate::repository::db::run_migrations;
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:").await.unwrap();
        run_migrations(&pool).await.unwrap();

        let tenant_id = Uuid::new_v4();
        let speaker_id = Uuid::new_v4();
        // Seed a profile with aliases + bio.
        sqlx::query(
            r#"INSERT INTO tenant_profile (tenant_id, display_name, aliases, bio)
               VALUES (?1, 'Old Name', '["DK","彭大可"]', 'I build things.')"#,
        ).bind(tenant_id.to_string()).execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO speakers (id, tenant_id, display_name) VALUES (?1, ?2, 'Dake')",
        ).bind(speaker_id.to_string()).bind(tenant_id.to_string())
        .execute(&pool).await.unwrap();

        super::mark_as_self(&pool, speaker_id).await.unwrap();

        let row: (String, String, Option<String>) = sqlx::query_as(
            "SELECT display_name, aliases, bio FROM tenant_profile WHERE tenant_id = ?1"
        ).bind(tenant_id.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(row.0, "Dake", "display_name should be overwritten by the speaker name");
        assert_eq!(row.1, r#"["DK","彭大可"]"#, "aliases must be preserved");
        assert_eq!(row.2.as_deref(), Some("I build things."), "bio must be preserved");
    }

    #[tokio::test]
    async fn partial_unique_index_blocks_two_self_speakers_per_tenant() {
        let pool = fresh_pool().await;

        let tenant_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO speakers (id, tenant_id, display_name, is_self) VALUES (?1, ?2, 'A', 1)",
        )
        .bind(a.to_string())
        .bind(tenant_id.to_string())
        .execute(&pool)
        .await
        .unwrap();

        let result = sqlx::query(
            "INSERT INTO speakers (id, tenant_id, display_name, is_self) VALUES (?1, ?2, 'B', 1)",
        )
        .bind(b.to_string())
        .bind(tenant_id.to_string())
        .execute(&pool)
        .await;
        assert!(result.is_err(), "expected unique index violation");
    }
}
