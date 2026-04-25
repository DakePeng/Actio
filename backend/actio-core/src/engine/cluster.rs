//! Agglomerative hierarchical clustering over speaker embeddings.
//!
//! Pure function: input is `(segment_id, embedding)` pairs and a cosine
//! distance threshold; output is a stable cluster index per input. Used by
//! the batch processor to derive per-clip speaker tracks.
//!
//! Algorithm: average-linkage AHC on cosine distance between cluster
//! centroids. Stops merging when the smallest pair distance exceeds
//! `threshold`. O(n^2 log n) but n ≤ a few hundred per 5-min clip — well
//! within budget.

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterAssignment {
    pub segment_id: Uuid,
    pub cluster_idx: usize,
}

/// Cluster `inputs` into 0-indexed buckets using cosine-distance AHC.
/// Two inputs whose unit-normalized embeddings have cosine distance
/// ≤ `cosine_distance_threshold` (i.e. similarity ≥ `1 - threshold`)
/// will end up in the same cluster, transitively through average-linkage.
///
/// Cluster indices in the returned vector are compact (`0..k`) and in
/// first-seen order — the segment with the smallest input index inside
/// cluster id `c` is the one whose own cluster_idx ≤ all later c-members.
pub fn ahc(
    inputs: &[(Uuid, Vec<f32>)],
    cosine_distance_threshold: f32,
) -> Vec<ClusterAssignment> {
    if inputs.is_empty() {
        return Vec::new();
    }
    if inputs.len() == 1 {
        return vec![ClusterAssignment { segment_id: inputs[0].0, cluster_idx: 0 }];
    }

    let n = inputs.len();
    let mut membership: Vec<usize> = (0..n).collect();
    let mut active: std::collections::BTreeSet<usize> = (0..n).collect();
    let mut sizes: Vec<usize> = vec![1; n];
    let mut centroids: Vec<Vec<f32>> =
        inputs.iter().map(|(_, v)| normalized(v)).collect();

    loop {
        let actives: Vec<usize> = active.iter().copied().collect();
        let mut best: Option<(f32, usize, usize)> = None;
        for i in 0..actives.len() {
            for j in (i + 1)..actives.len() {
                let a = actives[i];
                let b = actives[j];
                let d = 1.0 - cosine_sim(&centroids[a], &centroids[b]);
                if best.map_or(true, |(bd, _, _)| d < bd) {
                    best = Some((d, a, b));
                }
            }
        }
        match best {
            Some((d, a, b)) if d <= cosine_distance_threshold => {
                let new_centroid = weighted_mean(
                    &centroids[a], sizes[a],
                    &centroids[b], sizes[b],
                );
                centroids[a] = normalized(&new_centroid);
                sizes[a] += sizes[b];
                for m in membership.iter_mut() {
                    if *m == b {
                        *m = a;
                    }
                }
                active.remove(&b);
            }
            _ => break,
        }
    }

    // Compact membership into 0..k in first-seen order.
    let mut compact: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut next_idx = 0usize;
    for &m in membership.iter() {
        compact.entry(m).or_insert_with(|| {
            let idx = next_idx;
            next_idx += 1;
            idx
        });
    }
    inputs
        .iter()
        .enumerate()
        .map(|(i, (id, _))| ClusterAssignment {
            segment_id: *id,
            cluster_idx: compact[&membership[i]],
        })
        .collect()
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>()
}

fn norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

fn normalized(v: &[f32]) -> Vec<f32> {
    let n = norm(v);
    if n < 1e-8 {
        v.to_vec()
    } else {
        v.iter().map(|x| x / n).collect()
    }
}

fn weighted_mean(a: &[f32], an: usize, b: &[f32], bn: usize) -> Vec<f32> {
    let total = (an + bn) as f32;
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (*x * an as f32 + *y * bn as f32) / total)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }

    #[test]
    fn empty_input_returns_empty() {
        assert!(ahc(&[], 0.4).is_empty());
    }

    #[test]
    fn single_input_returns_one_cluster() {
        let out = ahc(&[(id(1), vec![1.0, 0.0])], 0.4);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].cluster_idx, 0);
    }

    #[test]
    fn two_orthogonal_vectors_are_two_clusters() {
        let inputs = vec![(id(1), vec![1.0, 0.0]), (id(2), vec![0.0, 1.0])];
        let out = ahc(&inputs, 0.4);
        assert_eq!(out[0].cluster_idx, 0);
        assert_eq!(out[1].cluster_idx, 1);
    }

    #[test]
    fn two_collinear_vectors_collapse_into_one_cluster() {
        let inputs = vec![(id(1), vec![1.0, 0.0]), (id(2), vec![0.999, 0.044])];
        let out = ahc(&inputs, 0.4);
        assert_eq!(out[0].cluster_idx, out[1].cluster_idx);
    }

    #[test]
    fn three_speakers_two_clusters_each_resolves_correctly() {
        let inputs = vec![
            (id(1), vec![1.0, 0.0, 0.0]),
            (id(2), vec![0.99, 0.14, 0.0]),    // A
            (id(3), vec![0.0, 1.0, 0.0]),
            (id(4), vec![0.14, 0.99, 0.0]),    // B
            (id(5), vec![0.0, 0.0, 1.0]),       // C
        ];
        let out = ahc(&inputs, 0.4);
        assert_eq!(out[0].cluster_idx, out[1].cluster_idx);
        assert_eq!(out[2].cluster_idx, out[3].cluster_idx);
        assert_ne!(out[0].cluster_idx, out[2].cluster_idx);
        assert_ne!(out[0].cluster_idx, out[4].cluster_idx);
        assert_ne!(out[2].cluster_idx, out[4].cluster_idx);
    }

    #[test]
    fn stable_cluster_indices_are_compact_zero_indexed() {
        let inputs = vec![
            (id(1), vec![1.0, 0.0]),
            (id(2), vec![0.0, 1.0]),
            (id(3), vec![0.99, 0.14]),
        ];
        let out = ahc(&inputs, 0.4);
        let max_idx = out.iter().map(|a| a.cluster_idx).max().unwrap();
        assert!(max_idx < out.len());
        let used: std::collections::BTreeSet<_> =
            out.iter().map(|a| a.cluster_idx).collect();
        assert_eq!(used.len(), max_idx + 1);
    }

    #[test]
    fn first_seen_ordering_assigns_cluster_zero_to_first_input() {
        let inputs = vec![
            (id(1), vec![1.0, 0.0]),
            (id(2), vec![0.0, 1.0]),
            (id(3), vec![0.99, 0.14]),
        ];
        let out = ahc(&inputs, 0.4);
        assert_eq!(out[0].cluster_idx, 0, "first input should land in cluster 0");
    }
}
