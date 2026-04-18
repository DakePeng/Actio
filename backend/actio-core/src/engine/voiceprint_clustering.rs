//! Phase B: cluster retained voiceprint-candidate segments by embedding
//! similarity so the eventual "who was this?" prompt can ask once per
//! distinct unknown voice instead of once per segment.
//!
//! Algorithm: single-linkage agglomerative clustering with a cosine-similarity
//! threshold. For each incoming segment we find the existing cluster whose
//! centroid is most similar; if that similarity clears `threshold` we join it,
//! otherwise we open a new cluster. With realistic volumes (a few hundred
//! candidates per day) the O(N×K) cost is negligible.

use crate::engine::diarization::cosine_similarity;

/// One segment waiting to be clustered. Fields mirror what the candidates
/// endpoint needs for the final response plus the embedding for clustering.
#[derive(Debug, Clone)]
pub struct CandidateSegment {
    pub id: String,
    pub session_id: String,
    pub audio_ref: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct CandidateCluster {
    /// The longest-duration member, used as the audio preview.
    pub representative: CandidateSegment,
    pub member_ids: Vec<String>,
    pub occurrences: usize,
    pub total_duration_ms: i64,
    pub earliest_ms: i64,
    pub latest_ms: i64,
    /// Number of distinct `session_id`s across cluster members. A cluster
    /// heard in many sessions is more likely to be a person the user
    /// actually interacts with rather than a one-off voice (podcast,
    /// caller, background chatter).
    pub distinct_sessions: usize,
}

/// Evidence bar for "this is probably a person worth asking about."
/// Tuned conservatively so the prompt modal doesn't get annoying.
#[derive(Debug, Clone, Copy)]
pub struct PromptEligibility {
    pub min_occurrences: usize,
    pub min_total_duration_ms: i64,
    pub min_distinct_sessions: usize,
}

impl Default for PromptEligibility {
    fn default() -> Self {
        Self {
            min_occurrences: 5,
            min_total_duration_ms: 60_000,
            min_distinct_sessions: 2,
        }
    }
}

impl PromptEligibility {
    pub fn passes(&self, c: &CandidateCluster) -> bool {
        c.occurrences >= self.min_occurrences
            && c.total_duration_ms >= self.min_total_duration_ms
            && c.distinct_sessions >= self.min_distinct_sessions
    }
}

/// Empirically tuned for ERes2Net 512-dim embeddings. Two clips of the same
/// speaker under similar conditions typically score > 0.6; different speakers
/// cluster well below 0.4. 0.5 is a conservative "probably same person" cut.
pub const DEFAULT_CLUSTER_THRESHOLD: f32 = 0.5;

/// Single-linkage agglomerative clustering. Input does not need to be
/// pre-sorted; output clusters are ordered by `occurrences` descending
/// (most-heard voices first) so the UI can prompt about them in priority order.
pub fn cluster_candidates(
    segments: Vec<CandidateSegment>,
    threshold: f32,
) -> Vec<CandidateCluster> {
    // Accumulator: each entry holds the running centroid, members, and the
    // stats we'll expose on CandidateCluster.
    struct Bucket {
        centroid: Vec<f32>,
        centroid_n: usize,
        rep_idx: usize, // index into members
        members: Vec<CandidateSegment>,
    }
    let mut buckets: Vec<Bucket> = Vec::new();

    for seg in segments {
        // Find the best-matching existing bucket.
        let mut best: Option<(usize, f32)> = None;
        for (bi, b) in buckets.iter().enumerate() {
            let sim = cosine_similarity(&b.centroid, &seg.embedding);
            if best.map_or(true, |(_, s)| sim > s) {
                best = Some((bi, sim));
            }
        }

        match best {
            Some((bi, sim)) if sim >= threshold => {
                let b = &mut buckets[bi];
                // Update running mean: new_mean = old_mean + (x - old_mean) / (n+1)
                let new_n = b.centroid_n + 1;
                for (slot, x) in b.centroid.iter_mut().zip(seg.embedding.iter()) {
                    *slot += (*x - *slot) / (new_n as f32);
                }
                b.centroid_n = new_n;
                // Update representative if this segment is longer.
                let seg_dur = seg.end_ms - seg.start_ms;
                let rep_dur = b.members[b.rep_idx].end_ms - b.members[b.rep_idx].start_ms;
                if seg_dur > rep_dur {
                    b.rep_idx = b.members.len();
                }
                b.members.push(seg);
            }
            _ => {
                buckets.push(Bucket {
                    centroid: seg.embedding.clone(),
                    centroid_n: 1,
                    rep_idx: 0,
                    members: vec![seg],
                });
            }
        }
    }

    let mut clusters: Vec<CandidateCluster> = buckets
        .into_iter()
        .map(|b| {
            let occurrences = b.members.len();
            let total_duration_ms = b.members.iter().map(|m| m.end_ms - m.start_ms).sum();
            let earliest_ms = b.members.iter().map(|m| m.start_ms).min().unwrap_or(0);
            let latest_ms = b.members.iter().map(|m| m.end_ms).max().unwrap_or(0);
            let member_ids = b.members.iter().map(|m| m.id.clone()).collect();
            let distinct_sessions = {
                let mut set = std::collections::HashSet::new();
                for m in &b.members {
                    set.insert(m.session_id.clone());
                }
                set.len()
            };
            let representative = b.members.into_iter().nth(b.rep_idx).expect("rep_idx valid");
            CandidateCluster {
                representative,
                member_ids,
                occurrences,
                total_duration_ms,
                earliest_ms,
                latest_ms,
                distinct_sessions,
            }
        })
        .collect();

    clusters.sort_by(|a, b| b.occurrences.cmp(&a.occurrences));
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: &str, emb: Vec<f32>, dur_ms: i64) -> CandidateSegment {
        CandidateSegment {
            id: id.to_string(),
            session_id: "s".to_string(),
            audio_ref: format!("{id}.wav"),
            start_ms: 0,
            end_ms: dur_ms,
            embedding: emb,
        }
    }

    fn normalize(v: &mut [f32]) {
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n > 0.0 {
            for x in v {
                *x /= n;
            }
        }
    }

    #[test]
    fn similar_embeddings_cluster_together() {
        let mut a1 = vec![1.0; 64];
        a1[0] = 5.0;
        normalize(&mut a1);
        let mut a2 = vec![1.0; 64];
        a2[0] = 4.5; // close to a1
        normalize(&mut a2);
        let mut a3 = vec![1.0; 64];
        a3[0] = 5.5;
        normalize(&mut a3);

        let clusters = cluster_candidates(
            vec![seg("a1", a1, 5000), seg("a2", a2, 4000), seg("a3", a3, 6000)],
            0.5,
        );
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].occurrences, 3);
        // Longest member is a3 at 6000ms.
        assert_eq!(clusters[0].representative.id, "a3");
        assert_eq!(clusters[0].total_duration_ms, 15000);
    }

    #[test]
    fn orthogonal_embeddings_form_separate_clusters() {
        let mut alice = vec![0.01; 64];
        alice[0] = 1.0;
        normalize(&mut alice);
        let mut bob = vec![0.01; 64];
        bob[32] = 1.0;
        normalize(&mut bob);

        let clusters = cluster_candidates(
            vec![seg("alice1", alice.clone(), 3000), seg("bob1", bob, 3000)],
            0.5,
        );
        assert_eq!(clusters.len(), 2);
        assert!(clusters.iter().all(|c| c.occurrences == 1));
    }

    #[test]
    fn clusters_sorted_by_occurrences_descending() {
        let mut alice = vec![0.01; 64];
        alice[0] = 1.0;
        normalize(&mut alice);
        let mut bob = vec![0.01; 64];
        bob[32] = 1.0;
        normalize(&mut bob);

        // 3 alice clips, 1 bob clip → alice cluster first.
        let segs = vec![
            seg("a1", alice.clone(), 3000),
            seg("b1", bob.clone(), 3000),
            seg("a2", alice.clone(), 3000),
            seg("a3", alice, 3000),
        ];
        let clusters = cluster_candidates(segs, 0.5);
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].occurrences, 3);
        assert_eq!(clusters[1].occurrences, 1);
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(cluster_candidates(vec![], 0.5).is_empty());
    }

    #[test]
    fn single_segment_forms_singleton_cluster() {
        let mut emb = vec![0.01; 64];
        emb[0] = 1.0;
        normalize(&mut emb);
        let clusters = cluster_candidates(vec![seg("only", emb, 5000)], 0.5);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].occurrences, 1);
        assert_eq!(clusters[0].representative.id, "only");
    }

    #[test]
    fn eligibility_gate_rejects_below_thresholds() {
        let gate = PromptEligibility::default();
        let mut emb = vec![0.01; 64];
        emb[0] = 1.0;
        normalize(&mut emb);
        let clusters = cluster_candidates(vec![seg("only", emb, 5000)], 0.5);
        assert!(!gate.passes(&clusters[0]));
    }

    #[test]
    fn eligibility_gate_accepts_strong_evidence() {
        let gate = PromptEligibility::default();
        let mut emb = vec![0.01; 64];
        emb[0] = 1.0;
        normalize(&mut emb);
        // 6 clips @ 12s each = 72s across 2 sessions → passes all gates.
        let segs: Vec<_> = (0..6)
            .map(|i| {
                let mut s = seg(&format!("s{i}"), emb.clone(), 12_000);
                s.session_id = if i < 3 { "sess-a".into() } else { "sess-b".into() };
                s
            })
            .collect();
        let clusters = cluster_candidates(segs, 0.5);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].distinct_sessions, 2);
        assert!(gate.passes(&clusters[0]));
    }
}
