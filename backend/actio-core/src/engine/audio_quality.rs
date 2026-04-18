/// Produce a heuristic quality score in [0, 1] from a 16 kHz mono f32 clip.
/// High score = louder, cleaner, longer. Low score = clipped, silent, or too short.
pub fn score(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let duration_s = samples.len() as f32 / 16_000.0;
    let rms = (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt();

    // RMS target band: 0.05 .. 0.30 is "good". Outside that, score drops.
    let rms_term = if rms < 0.01 {
        0.0
    } else if rms < 0.05 {
        (rms - 0.01) / 0.04
    } else if rms <= 0.30 {
        1.0
    } else if rms <= 0.60 {
        1.0 - (rms - 0.30) / 0.30
    } else {
        0.0
    };

    // Silence or clipping are disqualifying — a long silent clip should still
    // score zero, not 0.3 for duration alone.
    if rms_term == 0.0 {
        return 0.0;
    }

    // Duration target: >=8s is ideal, 3s is bare minimum.
    let dur_term = if duration_s < 3.0 {
        0.0
    } else if duration_s >= 8.0 {
        1.0
    } else {
        (duration_s - 3.0) / 5.0
    };

    0.7 * rms_term + 0.3 * dur_term
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_scores_zero() {
        assert_eq!(score(&vec![0.0; 16_000 * 10]), 0.0);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(score(&[]), 0.0);
    }

    #[test]
    fn clipped_loud_scores_low() {
        let s = score(&vec![0.95; 16_000 * 10]);
        assert!(s < 0.35, "got {s}");
    }

    #[test]
    fn good_loudness_and_duration_scores_high() {
        let samples: Vec<f32> = (0..16_000 * 10)
            .map(|i| (i as f32 * 0.01).sin() * 0.15)
            .collect();
        let s = score(&samples);
        assert!(s > 0.8, "got {s}");
    }

    #[test]
    fn short_clip_scores_lower() {
        let short: Vec<f32> = (0..16_000 * 3)
            .map(|i| (i as f32 * 0.01).sin() * 0.15)
            .collect();
        let s = score(&short);
        assert!(s < 0.8, "got {s}");
    }
}
