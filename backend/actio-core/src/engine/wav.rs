use std::io::Cursor;

/// Decode a WAV byte slice into 16 kHz mono f32 samples.
/// Accepts 8/16/24/32-bit PCM or 32-bit float; downmixes stereo to mono;
/// resamples any sample rate to 16 kHz via linear interpolation.
pub fn decode_to_mono_16k(bytes: &[u8]) -> anyhow::Result<(Vec<f32>, f64)> {
    let cursor = Cursor::new(bytes);
    let mut reader = hound::WavReader::new(cursor)?;
    let spec = reader.spec();

    // Collect samples as f32 in [-1, 1], averaged across channels.
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate as usize;

    let mono: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            let xs: Result<Vec<f32>, _> = reader.samples::<f32>().collect();
            fold_channels(&xs?, channels)
        }
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            let xs: Result<Vec<i32>, _> = reader.samples::<i32>().collect();
            let as_f32: Vec<f32> = xs?.into_iter().map(|s| s as f32 / max).collect();
            fold_channels(&as_f32, channels)
        }
    };

    let resampled = if sample_rate == 16_000 {
        mono
    } else {
        linear_resample(&mono, sample_rate, 16_000)
    };

    let duration_ms = (resampled.len() as f64 / 16_000.0) * 1000.0;
    Ok((resampled, duration_ms))
}

fn fold_channels(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn linear_resample(input: &[f32], src_rate: usize, dst_rate: usize) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = input[idx];
        let b = *input.get(idx + 1).unwrap_or(&a);
        out.push(a + (b - a) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};
    use std::io::Cursor;

    fn make_wav(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut buf = Cursor::new(Vec::<u8>::new());
        {
            let mut w = WavWriter::new(&mut buf, spec).unwrap();
            for s in samples {
                w.write_sample(*s).unwrap();
            }
            w.finalize().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn decodes_mono_16k_passthrough() {
        let wav = make_wav(&[0.1, -0.2, 0.3, -0.4], 16_000, 1);
        let (samples, dur) = decode_to_mono_16k(&wav).unwrap();
        assert_eq!(samples.len(), 4);
        assert!((dur - 0.25).abs() < 0.01);
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let wav = make_wav(&[1.0, -1.0, 0.5, -0.5], 16_000, 2);
        let (samples, _) = decode_to_mono_16k(&wav).unwrap();
        assert_eq!(samples.len(), 2);
        assert!((samples[0]).abs() < 0.001); // (1 + -1)/2
        assert!((samples[1]).abs() < 0.001); // (0.5 + -0.5)/2
    }

    #[test]
    fn resamples_48k_to_16k() {
        let len = 48_000; // 1s at 48k
        let samples: Vec<f32> = (0..len).map(|i| (i as f32).sin() * 0.1).collect();
        let wav = make_wav(&samples, 48_000, 1);
        let (out, dur) = decode_to_mono_16k(&wav).unwrap();
        assert!((out.len() as i32 - 16_000).abs() < 5);
        assert!((dur - 1000.0).abs() < 2.0);
    }
}
