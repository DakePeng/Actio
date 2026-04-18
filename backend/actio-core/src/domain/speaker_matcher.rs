use sqlx::SqlitePool;
use tracing::info;
use uuid::Uuid;

use crate::repository::speaker;

#[derive(Debug)]
pub struct SpeakerMatchResult {
    pub speaker_id: Option<Uuid>,
    pub similarity_score: f64,
    pub z_norm_score: f64,
    pub accepted: bool,
}

const Z_NORM_THRESHOLD: f64 = 0.0;

pub async fn identify_speaker(
    pool: &SqlitePool,
    embedding: &[f32],
    tenant_id: Uuid,
    k: usize,
) -> Result<SpeakerMatchResult, sqlx::Error> {
    let speakers = speaker::list_speakers(pool, tenant_id).await?;
    if speakers.is_empty() {
        return Ok(SpeakerMatchResult {
            speaker_id: None,
            similarity_score: 0.0,
            z_norm_score: 0.0,
            accepted: false,
        });
    }

    let emb_str: String = embedding
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let raw_results: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT e.speaker_id, 1.0 - (e.embedding_distance) AS similarity \
         FROM speaker_embeddings e \
         JOIN speakers s ON s.id = e.speaker_id \
         WHERE s.tenant_id = ?1 AND s.status = 'active' \
         ORDER BY e.embedding_distance LIMIT ?2",
    )
    .bind(format!("[{}]", emb_str))
    .bind(k as i64)
    .fetch_all(pool)
    .await?;

    let similarities: Vec<f64> = raw_results.iter().map(|(_, s)| *s).collect();
    let (mean, std_dev) = compute_stats(&similarities);
    let z_scores: Vec<f64> = if std_dev > 0.001 {
        similarities.iter().map(|s| (s - mean) / std_dev).collect()
    } else {
        similarities.iter().map(|_| 0.0).collect()
    };

    let best_idx = z_scores
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);

    if let Some(idx) = best_idx {
        let (speaker_id, sim) = raw_results[idx];
        let z_norm = z_scores[idx];
        let accepted = z_norm > Z_NORM_THRESHOLD;
        info!(?speaker_id, sim, z_norm, accepted, "Speaker identified");
        Ok(SpeakerMatchResult {
            speaker_id: accepted.then_some(speaker_id),
            similarity_score: sim,
            z_norm_score: z_norm,
            accepted,
        })
    } else {
        Ok(SpeakerMatchResult {
            speaker_id: None,
            similarity_score: 0.0,
            z_norm_score: 0.0,
            accepted: false,
        })
    }
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
    use crate::repository::db::run_migrations;
    use crate::repository::speaker::create_speaker;
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
}
