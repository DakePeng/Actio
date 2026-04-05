use sqlx::PgPool;
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
    pool: &PgPool,
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

    let emb_str: String = embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    let raw_results: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT e.speaker_id, 1 - (e.embedding <=> $1::vector) AS similarity \
         FROM speaker_embeddings e \
         JOIN speakers s ON s.id = e.speaker_id \
         WHERE s.tenant_id = $2 AND s.status = 'active' \
         ORDER BY e.embedding <=> $1::vector LIMIT $3",
    )
    .bind(format!("[{}]", emb_str))
    .bind(tenant_id)
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
    pool: &PgPool,
    speaker_id: Uuid,
    embedding: &[f32],
    duration_ms: f64,
    quality_score: f64,
    is_primary: bool,
) -> Result<Uuid, sqlx::Error> {
    let emb_str: String = embedding.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
    let vector_str = format!("[{}]", emb_str);

    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO speaker_embeddings (speaker_id, embedding, duration_ms, quality_score, is_primary, embedding_dimension) \
         VALUES ($1, $2::vector, $3, $4, $5, 192) RETURNING id",
    )
    .bind(speaker_id)
    .bind(vector_str)
    .bind(duration_ms)
    .bind(quality_score)
    .bind(is_primary)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
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
}
