use sqlx::SqlitePool;
use tracing::info;
use uuid::Uuid;

use crate::engine::diarization::cosine_similarity;

/// Confidence tier for a speaker match. Used both in `SpeakerMatchResult`
/// (returned by `identify_speaker_with_thresholds`) and in the continuity
/// state machine's `AttributionOutcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchConfidence {
    Confirmed,
    Tentative,
}

impl MatchConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchConfidence::Confirmed => "confirmed",
            MatchConfidence::Tentative => "tentative",
        }
    }
}

/// Thresholds for `identify_speaker_with_thresholds`. Values are cosine
/// similarity scores in [0, 1]. `confirm` must be ≥ `tentative`.
#[derive(Debug, Clone, Copy)]
pub struct IdentifyThresholds {
    pub confirm: f64,
    pub tentative: f64,
}

#[derive(Debug)]
pub struct SpeakerMatchResult {
    pub speaker_id: Option<String>,
    pub similarity_score: f64,
    pub z_norm_score: f64,
    pub accepted: bool,
    pub confidence: Option<MatchConfidence>,
}

const Z_NORM_THRESHOLD: f64 = 0.0;
// Guard against dividing by near-zero std dev (e.g., all candidates equidistant).
const MIN_STD_DEV: f64 = 0.001;
// Absolute cosine threshold used when Z-norm cannot discriminate (single
// candidate, or all candidates tied). Picked as a conservative default for
// ERes2Net-style 512-dim embeddings — expected to be tuned with usage data.
const SINGLE_CANDIDATE_COSINE_THRESHOLD: f64 = 0.5;

pub async fn identify_speaker(
    pool: &SqlitePool,
    query: &[f32],
    tenant_id: Uuid,
    k: usize,
) -> Result<SpeakerMatchResult, sqlx::Error> {
    // Fetch all embeddings for active speakers in the tenant whose dimension
    // matches the query. We sort by similarity in Rust; dataset is small
    // (hundreds of rows at most in realistic usage) and pgvector is unavailable.
    let query_dim = query.len() as i64;

    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT e.speaker_id, e.embedding \
         FROM speaker_embeddings e \
         JOIN speakers s ON s.id = e.speaker_id \
         WHERE s.tenant_id = ?1 \
           AND s.status = 'active' \
           AND e.embedding_dimension = ?2",
    )
    .bind(tenant_id.to_string())
    .bind(query_dim)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(SpeakerMatchResult {
            speaker_id: None,
            similarity_score: 0.0,
            z_norm_score: 0.0,
            accepted: false,
            confidence: None,
        });
    }

    // Compute cosine similarity for every candidate.
    // SQLite's Vec<u8> allocations are 8-byte aligned on all supported targets,
    // so bytemuck::cast_slice::<u8, f32> is safe in practice. The debug assert
    // makes an accidental misalignment fail loudly in dev rather than silently
    // in production.
    let mut scored: Vec<(String, f64)> = rows
        .into_iter()
        .map(|(speaker_id, blob)| {
            debug_assert_eq!(
                blob.as_ptr() as usize % std::mem::align_of::<f32>(),
                0,
                "embedding BLOB not 4-byte aligned"
            );
            let emb: &[f32] = bytemuck::cast_slice(&blob);
            let sim = cosine_similarity(query, emb) as f64;
            (speaker_id, sim)
        })
        .collect();

    // Keep top-k
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);

    let sims: Vec<f64> = scored.iter().map(|(_, s)| *s).collect();
    let (mean, std_dev) = compute_stats(&sims);
    let z_scores: Vec<f64> = if std_dev > MIN_STD_DEV {
        sims.iter().map(|s| (s - mean) / std_dev).collect()
    } else {
        sims.iter().map(|_| 0.0).collect()
    };

    let best_idx = z_scores
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);

    if let Some(idx) = best_idx {
        let z_norm = z_scores[idx];
        let (speaker_id, sim) = scored.swap_remove(idx);
        // Z-norm requires ≥2 candidates with variance to discriminate. When
        // std_dev collapsed to ~0 (single candidate, or all-tied), fall back
        // to an absolute cosine threshold so the only enrolled speaker can
        // still be accepted.
        let accepted = if std_dev > MIN_STD_DEV {
            z_norm > Z_NORM_THRESHOLD
        } else {
            sim >= SINGLE_CANDIDATE_COSINE_THRESHOLD
        };
        info!(speaker_id = %speaker_id, sim, z_norm, accepted, "Speaker identified");
        Ok(SpeakerMatchResult {
            speaker_id: accepted.then_some(speaker_id),
            similarity_score: sim,
            z_norm_score: z_norm,
            accepted,
            confidence: accepted.then_some(MatchConfidence::Confirmed),
        })
    } else {
        Ok(SpeakerMatchResult {
            speaker_id: None,
            similarity_score: 0.0,
            z_norm_score: 0.0,
            accepted: false,
            confidence: None,
        })
    }
}

/// Threshold-based variant used by the continuity pipeline. Same scoring
/// as `identify_speaker` but decision is based on absolute cosine against
/// the caller-supplied confirm/tentative thresholds, and the returned
/// `confidence` tier drives the downstream state machine.
pub async fn identify_speaker_with_thresholds(
    pool: &SqlitePool,
    query: &[f32],
    tenant_id: Uuid,
    thresholds: IdentifyThresholds,
) -> Result<SpeakerMatchResult, sqlx::Error> {
    let query_dim = query.len() as i64;
    let rows: Vec<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT e.speaker_id, e.embedding \
         FROM speaker_embeddings e \
         JOIN speakers s ON s.id = e.speaker_id \
         WHERE s.tenant_id = ?1 \
           AND s.status = 'active' \
           AND e.embedding_dimension = ?2",
    )
    .bind(tenant_id.to_string())
    .bind(query_dim)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(SpeakerMatchResult {
            speaker_id: None,
            similarity_score: 0.0,
            z_norm_score: 0.0,
            accepted: false,
            confidence: None,
        });
    }

    let mut scored: Vec<(String, f64)> = rows
        .into_iter()
        .map(|(speaker_id, blob)| {
            debug_assert_eq!(
                blob.as_ptr() as usize % std::mem::align_of::<f32>(),
                0,
                "embedding BLOB not 4-byte aligned"
            );
            let emb: &[f32] = bytemuck::cast_slice(&blob);
            (speaker_id, cosine_similarity(query, emb) as f64)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let (best_id, best_sim) = scored[0].clone();
    let confidence = if best_sim >= thresholds.confirm {
        Some(MatchConfidence::Confirmed)
    } else if best_sim >= thresholds.tentative {
        Some(MatchConfidence::Tentative)
    } else {
        None
    };
    let accepted = confidence.is_some();
    info!(
        speaker_id = %best_id,
        sim = best_sim,
        confirm = thresholds.confirm,
        tentative = thresholds.tentative,
        ?confidence,
        "Speaker identified (thresholded)"
    );

    Ok(SpeakerMatchResult {
        speaker_id: accepted.then_some(best_id),
        similarity_score: best_sim,
        z_norm_score: 0.0,
        accepted,
        confidence,
    })
}

pub async fn save_embedding(
    pool: &SqlitePool,
    speaker_id: Uuid,
    embedding: &[f32],
    duration_ms: f64,
    quality_score: f64,
    is_primary: bool,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4().to_string();
    let blob: &[u8] = bytemuck::cast_slice(embedding);
    let dim = embedding.len() as i64;

    let row: (String,) = sqlx::query_as(
        "INSERT INTO speaker_embeddings \
           (id, speaker_id, embedding, duration_ms, quality_score, is_primary, embedding_dimension) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(&id)
    .bind(speaker_id.to_string())
    .bind(blob)
    .bind(duration_ms)
    .bind(quality_score)
    .bind(is_primary as i64)
    .bind(dim)
    .fetch_one(pool)
    .await?;

    Uuid::parse_str(&row.0).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

fn compute_stats(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    if n == 0.0 {
        return (0.0, 0.0);
    }
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, variance.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::speaker::create_speaker;

    use crate::testing::fresh_pool;

    #[test]
    fn test_compute_stats() {
        let (mean, std) = compute_stats(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!((mean - 3.0).abs() < 0.001);
        assert!((std - 1.414).abs() < 0.01);
    }

    #[test]
    fn test_empty_stats() {
        let (mean, std) = compute_stats(&[]);
        assert_eq!(mean, 0.0);
        assert_eq!(std, 0.0);
    }

    #[tokio::test]
    async fn save_and_load_embedding_roundtrip() {
        let pool = fresh_pool().await;
        let s = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let sid = Uuid::parse_str(&s.id).unwrap();

        // Arbitrary 512-dim embedding with recognisable bit patterns
        let emb: Vec<f32> = (0..512).map(|i| i as f32 / 100.0).collect();
        let id = save_embedding(&pool, sid, &emb, 10_000.0, 0.8, true)
            .await
            .unwrap();
        assert_ne!(id, Uuid::nil());

        // Read back via a direct query and decode
        let row: (Vec<u8>, i64) = sqlx::query_as(
            "SELECT embedding, embedding_dimension FROM speaker_embeddings \
             WHERE speaker_id = ?1",
        )
        .bind(sid.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.1, 512);
        let decoded: &[f32] = bytemuck::cast_slice(&row.0);
        assert_eq!(decoded, emb.as_slice());
    }

    fn normalize(v: &mut [f32]) {
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n > 0.0 {
            for x in v {
                *x /= n;
            }
        }
    }

    #[tokio::test]
    async fn identify_picks_closest_above_threshold() {
        let pool = fresh_pool().await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let bob = create_speaker(&pool, "Bob", "#64B5F6", Uuid::nil())
            .await
            .unwrap();

        // Alice: embedding close to [1, 0, 0, ...]
        let mut alice_emb = vec![1.0; 512];
        for i in 1..512 {
            alice_emb[i] = 0.01;
        }
        normalize(&mut alice_emb);

        // Bob: embedding close to [0, 1, 0, ...]
        let mut bob_emb = vec![0.01; 512];
        bob_emb[1] = 1.0;
        normalize(&mut bob_emb);

        save_embedding(
            &pool,
            Uuid::parse_str(&alice.id).unwrap(),
            &alice_emb,
            5000.0,
            0.9,
            true,
        )
        .await
        .unwrap();
        save_embedding(
            &pool,
            Uuid::parse_str(&bob.id).unwrap(),
            &bob_emb,
            5000.0,
            0.9,
            true,
        )
        .await
        .unwrap();

        // Query close to Alice
        let mut query = alice_emb.clone();
        query[2] = 0.02;
        normalize(&mut query);

        let result = identify_speaker(&pool, &query, Uuid::nil(), 5)
            .await
            .unwrap();
        assert!(result.accepted);
        assert_eq!(result.speaker_id.as_deref(), Some(alice.id.as_str()));
    }

    #[tokio::test]
    async fn identify_accepts_single_enrolled_speaker_by_absolute_similarity() {
        // Regression: with one candidate std_dev is 0 so z-norm alone cannot
        // accept anyone. Absolute cosine fallback must kick in.
        let pool = fresh_pool().await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let mut emb = vec![0.01f32; 512];
        emb[0] = 1.0;
        normalize(&mut emb);
        save_embedding(
            &pool,
            Uuid::parse_str(&alice.id).unwrap(),
            &emb,
            5000.0,
            0.9,
            true,
        )
        .await
        .unwrap();

        // Query identical to Alice's embedding → cosine ≈ 1.0, well above 0.5.
        let result = identify_speaker(&pool, &emb, Uuid::nil(), 5).await.unwrap();
        assert!(
            result.accepted,
            "single enrolled speaker should be accepted"
        );
        assert_eq!(result.speaker_id.as_deref(), Some(alice.id.as_str()));
        assert!(result.similarity_score >= 0.5);
    }

    #[tokio::test]
    async fn identify_rejects_single_low_similarity() {
        // Single candidate but the query is orthogonal — cosine ~0, below 0.5.
        let pool = fresh_pool().await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        let mut alice_emb = vec![0.0f32; 512];
        alice_emb[0] = 1.0;
        save_embedding(
            &pool,
            Uuid::parse_str(&alice.id).unwrap(),
            &alice_emb,
            5000.0,
            0.9,
            true,
        )
        .await
        .unwrap();

        let mut query = vec![0.0f32; 512];
        query[100] = 1.0;
        let result = identify_speaker(&pool, &query, Uuid::nil(), 5)
            .await
            .unwrap();
        assert!(!result.accepted);
        assert!(result.speaker_id.is_none());
    }

    #[tokio::test]
    async fn identify_ignores_wrong_dimension_rows() {
        let pool = fresh_pool().await;
        let alice = create_speaker(&pool, "Alice", "#E57373", Uuid::nil())
            .await
            .unwrap();
        // Insert a 192-dim embedding directly (simulating a stale row)
        sqlx::query(
            "INSERT INTO speaker_embeddings \
               (id, speaker_id, embedding, duration_ms, quality_score, is_primary, embedding_dimension) \
             VALUES (?1, ?2, ?3, 5000, 0.9, 1, 192)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&alice.id)
        .bind(bytemuck::cast_slice::<f32, u8>(&vec![0.5f32; 192]))
        .execute(&pool)
        .await
        .unwrap();

        let query = vec![0.5f32; 512];
        let result = identify_speaker(&pool, &query, Uuid::nil(), 5)
            .await
            .unwrap();
        // No 512-dim rows → no match, not a panic.
        assert!(!result.accepted);
        assert!(result.speaker_id.is_none());
    }
}
