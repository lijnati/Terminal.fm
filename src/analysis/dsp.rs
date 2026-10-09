//! Small, pure signal-processing helpers.

use realfft::num_complex::Complex;

/// Periodic Hann window of length `n`.
pub fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos()).collect()
}

/// Single-sided amplitude spectrum, scaled so a full-scale sine reads 1.0.
pub fn magnitudes(spectrum: &[Complex<f32>], window_sum: f32, out: &mut Vec<f32>) {
    out.clear();
    let scale = 2.0 / window_sum.max(f32::EPSILON);
    out.extend(spectrum.iter().map(|c| c.norm() * scale));
}

/// A frequency band, in Hz.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub lo: f32,
    pub hi: f32,
    pub center: f32,
}

/// `count` logarithmically spaced bands covering `min_hz..max_hz`.
pub fn log_bands(count: usize, min_hz: f32, max_hz: f32) -> Vec<Band> {
    if count == 0 || max_hz <= min_hz {
        return Vec::new();
    }
    let ratio = (max_hz / min_hz).ln() / count as f32;
    (0..count)
        .map(|i| {
            let lo = min_hz * (ratio * i as f32).exp();
            let hi = min_hz * (ratio * (i + 1) as f32).exp();
            Band { lo, hi, center: (lo * hi).sqrt() }
        })
        .collect()
}

/// Amplitude of `band`: the maximum bin inside it, or — when the band is narrower
/// than one bin (low frequencies) — linear interpolation at its center.
pub fn band_amplitude(mags: &[f32], band: &Band, sample_rate: u32, fft_size: usize) -> f32 {
    if mags.is_empty() || sample_rate == 0 {
        return 0.0;
    }
    let bin_hz = sample_rate as f32 / fft_size as f32;
    let last = mags.len() - 1;
    let lo = (band.lo / bin_hz).ceil() as usize;
    let hi = ((band.hi / bin_hz).floor() as usize).min(last);
    if lo <= hi {
        mags[lo..=hi].iter().cloned().fold(0.0, f32::max)
    } else {
        let pos = (band.center / bin_hz).min(last as f32);
        let i = pos.floor() as usize;
        let frac = pos - i as f32;
        let j = (i + 1).min(last);
        mags[i] * (1.0 - frac) + mags[j] * frac
    }
}

pub fn to_db(amp: f32) -> f32 {
    20.0 * amp.max(1e-9).log10()
}

/// Maps `db` from `floor..ceil` to 0.0–1.0, clamped. NaN maps to 0.
pub fn normalize_db(db: f32, floor: f32, ceil: f32) -> f32 {
    let v = (db - floor) / (ceil - floor);
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// +3 dB/octave tilt around 1 kHz, compensating music's natural high-frequency
/// roll-off so treble is visible. Zero at 1 kHz.
pub fn tilt_db(freq: f32) -> f32 {
    3.0 * (freq.max(1.0) / 1_000.0).log2()
}

/// Exponential smoothing with separate rise (`attack`) and fall (`release`) rates per second.
pub fn smooth(current: f32, target: f32, attack: f32, release: f32, dt: f32) -> f32 {
    let rate = if target > current { attack } else { release };
    let k = 1.0 - (-rate * dt).exp();
    let v = current + (target - current) * k;
    if v.is_finite() { v } else { 0.0 }
}

pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Reduces `samples` to `points` `(min, max)` pairs over equal slices — an
/// envelope that keeps transients and noise visible instead of averaging them away.
pub fn envelope_columns(samples: &[f32], points: usize) -> Vec<(f32, f32)> {
    if points == 0 || samples.is_empty() {
        return vec![(0.0, 0.0); points];
    }
    (0..points)
        .map(|p| {
            let a = p * samples.len() / points;
            let b = ((p + 1) * samples.len() / points).max(a + 1).min(samples.len());
            samples[a..b].iter().fold((f32::MAX, f32::MIN), |(lo, hi), &s| (lo.min(s), hi.max(s)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_is_zero_at_edges_and_one_at_center() {
        let w = hann(8);
        assert_eq!(w[0], 0.0);
        assert!((w[4] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn log_bands_are_contiguous_and_cover_range() {
        let b = log_bands(10, 40.0, 16_000.0);
        assert_eq!(b.len(), 10);
        assert!((b[0].lo - 40.0).abs() < 1e-3);
        assert!((b[9].hi - 16_000.0).abs() < 1.0);
        for w in b.windows(2) {
            assert!((w[0].hi - w[1].lo).abs() < 1e-3);
            // Equal ratio per band.
            assert!(((w[0].hi / w[0].lo) - (w[1].hi / w[1].lo)).abs() < 1e-3);
        }
        assert!(log_bands(0, 40.0, 100.0).is_empty());
        assert!(log_bands(4, 100.0, 40.0).is_empty());
    }

    #[test]
    fn band_amplitude_takes_max_or_interpolates() {
        // 100 Hz bins (sr 1000 / fft 10).
        let mags = vec![0.0, 0.2, 0.9, 0.1, 0.0, 0.0];
        let wide = Band { lo: 50.0, hi: 350.0, center: 132.0 };
        assert_eq!(band_amplitude(&mags, &wide, 1000, 10), 0.9);
        let narrow = Band { lo: 140.0, hi: 160.0, center: 150.0 };
        assert!((band_amplitude(&mags, &narrow, 1000, 10) - 0.55).abs() < 1e-6);
        assert_eq!(band_amplitude(&[], &wide, 1000, 10), 0.0);
    }

    #[test]
    fn db_normalization() {
        assert_eq!(normalize_db(-100.0, -72.0, -12.0), 0.0);
        assert_eq!(normalize_db(0.0, -72.0, -12.0), 1.0);
        assert!((normalize_db(-42.0, -72.0, -12.0) - 0.5).abs() < 1e-6);
        assert_eq!(normalize_db(f32::NAN, -72.0, -12.0), 0.0);
        assert!(to_db(0.0).is_finite());
    }

    #[test]
    fn tilt_is_zero_at_1k_and_3db_per_octave() {
        assert_eq!(tilt_db(1_000.0), 0.0);
        assert!((tilt_db(2_000.0) - 3.0).abs() < 1e-5);
        assert!((tilt_db(500.0) + 3.0).abs() < 1e-5);
    }

    #[test]
    fn smoothing_moves_toward_target_without_overshoot() {
        let up = smooth(0.0, 1.0, 10.0, 1.0, 0.1);
        assert!(up > 0.5 && up < 1.0);
        let down = smooth(1.0, 0.0, 10.0, 1.0, 0.1);
        assert!(down > 0.8, "release is slower than attack");
        assert_eq!(smooth(0.3, 0.3, 5.0, 5.0, 0.1), 0.3);
    }

    #[test]
    fn rms_of_known_signals() {
        assert_eq!(rms(&[]), 0.0);
        assert!((rms(&[0.5; 100]) - 0.5).abs() < 1e-6);
        assert!((rms(&[1.0, -1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn full_scale_sine_reads_unity() {
        use realfft::RealFftPlanner;
        let n = 1024;
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n);
        let w = hann(n);
        // Bin-centered frequency: 64 cycles over n samples.
        let mut input: Vec<f32> =
            (0..n).map(|i| (std::f32::consts::TAU * 64.0 * i as f32 / n as f32).sin() * w[i]).collect();
        let mut out = fft.make_output_vec();
        fft.process(&mut input, &mut out).unwrap();
        let mut mags = Vec::new();
        magnitudes(&out, w.iter().sum(), &mut mags);
        assert!((mags[64] - 1.0).abs() < 1e-3, "{}", mags[64]);
    }

    #[test]
    fn envelope_keeps_extremes() {
        assert_eq!(envelope_columns(&[1.0, -3.0, 0.5, 2.0], 2), vec![(-3.0, 1.0), (0.5, 2.0)]);
        assert_eq!(envelope_columns(&[1.0, 3.0], 4).len(), 4);
        assert_eq!(envelope_columns(&[], 3), vec![(0.0, 0.0); 3]);
    }
}
