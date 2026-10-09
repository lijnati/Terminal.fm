//! Real-time audio analysis for the visualizer.
//!
//! Operates on interleaved PCM tapped from the playback stream. Contains no
//! terminal or widget code: it produces normalized numbers (0.0–1.0 or −1.0–1.0)
//! that the UI maps to glyphs.

pub mod dsp;

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex};

pub use dsp::{Band, log_bands};

/// FFT length: 2048 samples ≈ 46 ms at 44.1 kHz, ≈ 21.5 Hz bin width.
pub const FFT_SIZE: usize = 2048;
/// Mono history kept for analysis (enough for one FFT plus waveform headroom).
const HISTORY: usize = FFT_SIZE * 2;
pub const MIN_FREQ: f32 = 40.0;
pub const MAX_FREQ: f32 = 16_000.0;
/// dB window mapped onto 0.0–1.0 bar height.
const FLOOR_DB: f32 = -72.0;
const CEIL_DB: f32 = -12.0;

/// What the analyzer currently knows about the incoming signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// No samples have arrived recently (paused, stopped, buffering, or no tap).
    None,
    /// Samples arrive but are effectively silent.
    Silence,
    Active,
}

/// Time-smoothed analysis output for one rendered frame.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// Band magnitudes, 0.0–1.0.
    pub bands: Vec<f32>,
    /// Peak-hold markers per band, 0.0–1.0.
    pub peaks: Vec<f32>,
    /// Smoothed RMS level, 0.0–1.0.
    pub rms: f32,
    /// Smoothed sample peak level, 0.0–1.0.
    pub peak: f32,
}

pub struct Analyzer {
    sample_rate: u32,
    /// Mono ring buffer of the most recent samples (oldest first after `ordered`).
    history: Vec<f32>,
    write: usize,
    filled: usize,
    /// Samples received since the last `update`.
    fresh: usize,
    /// Seconds since samples last arrived.
    starved_for: f32,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    window_sum: f32,
    fft_in: Vec<f32>,
    fft_out: Vec<Complex<f32>>,
    mags: Vec<f32>,
    bands: Vec<Band>,
    frame: Frame,
    peak_hold: Vec<f32>,
    wave_gain: f32,
    wave: Vec<(f32, f32)>,
    loud: bool,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    pub fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let window = dsp::hann(FFT_SIZE);
        let window_sum = window.iter().sum();
        Self {
            sample_rate: 44_100,
            history: vec![0.0; HISTORY],
            write: 0,
            filled: 0,
            fresh: 0,
            starved_for: f32::INFINITY,
            fft_in: fft.make_input_vec(),
            fft_out: fft.make_output_vec(),
            mags: vec![0.0; FFT_SIZE / 2 + 1],
            fft,
            window,
            window_sum,
            bands: Vec::new(),
            frame: Frame::default(),
            peak_hold: Vec::new(),
            wave_gain: 1.0,
            wave: Vec::new(),
            loud: false,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Appends interleaved samples, downmixing to mono. Non-finite samples become 0.
    pub fn push(&mut self, interleaved: &[f32], channels: u16, sample_rate: u32) {
        let ch = channels.max(1) as usize;
        if sample_rate > 0 {
            self.sample_rate = sample_rate;
        }
        for frame in interleaved.chunks_exact(ch) {
            let m = frame.iter().map(|s| if s.is_finite() { *s } else { 0.0 }).sum::<f32>() / ch as f32;
            self.history[self.write] = m;
            self.write = (self.write + 1) % HISTORY;
            self.filled = (self.filled + 1).min(HISTORY);
            self.fresh += 1;
        }
    }

    /// Discards history (e.g. after a seek or track change).
    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.filled = 0;
        self.fresh = 0;
    }

    /// The latest `n` mono samples, oldest first (zero-padded if fewer are available).
    fn latest(&self, n: usize, out: &mut Vec<f32>) {
        out.clear();
        let n = n.min(HISTORY);
        let pad = n.saturating_sub(self.filled);
        out.resize(pad, 0.0);
        let start = (self.write + HISTORY - (n - pad)) % HISTORY;
        for i in 0..(n - pad) {
            out.push(self.history[(start + i) % HISTORY]);
        }
    }

    pub fn signal(&self) -> Signal {
        if self.starved_for > 0.25 {
            Signal::None
        } else if !self.loud {
            Signal::Silence
        } else {
            Signal::Active
        }
    }

    /// Advances smoothing by `dt` seconds and recomputes `band_count` bands.
    /// `sensitivity` scales the signal (1.0 = neutral).
    pub fn update(&mut self, dt: f32, band_count: usize, sensitivity: f32) -> &Frame {
        let dt = dt.clamp(0.0, 0.5);
        if self.fresh == 0 {
            self.starved_for += dt;
        } else {
            self.starved_for = 0.0;
        }
        let have_signal = self.starved_for <= 0.25;
        self.fresh = 0;

        if self.bands.len() != band_count {
            // Fixed layout regardless of sample rate, so bars don't shift between tracks.
            self.bands = log_bands(band_count, MIN_FREQ, MAX_FREQ);
            self.frame.bands = vec![0.0; band_count];
            self.frame.peaks = vec![0.0; band_count];
            self.peak_hold = vec![0.0; band_count];
        }

        // Target values: zero when starved so bars fall naturally instead of freezing.
        let mut target = vec![0.0f32; band_count];
        let (mut rms, mut peak) = (0.0, 0.0);
        if have_signal {
            let mut buf = std::mem::take(&mut self.fft_in);
            self.latest(FFT_SIZE, &mut buf);
            rms = dsp::rms(&buf[FFT_SIZE - FFT_SIZE / 4..]);
            peak = buf[FFT_SIZE - FFT_SIZE / 4..].iter().fold(0.0f32, |a, s| a.max(s.abs()));
            for (s, w) in buf.iter_mut().zip(&self.window) {
                *s *= w;
            }
            self.fft_in = buf;
            if self.fft.process(&mut self.fft_in, &mut self.fft_out).is_ok() {
                dsp::magnitudes(&self.fft_out, self.window_sum, &mut self.mags);
                let gain_db = 20.0 * sensitivity.max(1e-3).log10();
                let nyquist = self.sample_rate as f32 / 2.0;
                for (t, band) in target.iter_mut().zip(&self.bands) {
                    if band.lo >= nyquist {
                        continue; // nothing up there at this sample rate
                    }
                    let amp = dsp::band_amplitude(&self.mags, band, self.sample_rate, FFT_SIZE);
                    let db = dsp::to_db(amp) + gain_db + dsp::tilt_db(band.center);
                    *t = dsp::normalize_db(db, FLOOR_DB, CEIL_DB);
                }
            }
        }
        self.loud = peak > 1e-4;

        let rows = self.frame.bands.iter_mut().zip(&mut self.frame.peaks).zip(&mut self.peak_hold);
        for (((v, peak), hold), t) in rows.zip(target) {
            *v = dsp::smooth(*v, t, 30.0, 6.0, dt);
            // Peaks hold briefly, then fall with gravity.
            if *v >= *peak {
                *peak = *v;
                *hold = 0.35;
            } else if *hold > 0.0 {
                *hold -= dt;
            } else {
                *peak = (*peak - 1.2 * dt).max(*v);
            }
        }
        let level = |x: f32| dsp::normalize_db(dsp::to_db(x * sensitivity), -60.0, 0.0);
        self.frame.rms = dsp::smooth(self.frame.rms, level(rms), 20.0, 4.0, dt);
        self.frame.peak = dsp::smooth(self.frame.peak, level(peak), 40.0, 3.0, dt);
        &self.frame
    }

    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    /// Waveform envelope for `points` horizontal positions as `(min, max)` in −1.0–1.0,
    /// aligned on a rising zero crossing so periodic signals stay still. Adaptive gain
    /// keeps quiet passages visible; `dt` drives the gain envelope and frame blending.
    pub fn waveform(&mut self, points: usize, dt: f32, sensitivity: f32) -> &[(f32, f32)] {
        let span = (FFT_SIZE / 2).max(points);
        let mut buf = Vec::with_capacity(span * 2);
        self.latest(span + span / 2, &mut buf);
        let starved = self.starved_for > 0.25;
        // Find a rising zero crossing in the first third for a stable trace.
        let search = span / 2;
        let offset = (1..search).find(|&i| buf[i - 1] < 0.0 && buf[i] >= 0.0).unwrap_or(search / 2);
        let window = &buf[offset..offset + span];

        let peak = window.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        // Aim for ~80% of the display height; slow to rise, quicker to back off.
        let want = if peak > 1e-4 { (0.8 / peak).clamp(1.0, 12.0) } else { self.wave_gain };
        let rate = if want < self.wave_gain { 8.0 } else { 1.5 };
        self.wave_gain += (want - self.wave_gain) * (1.0 - (-rate * dt.clamp(0.0, 0.5)).exp());

        let gain = self.wave_gain * sensitivity;
        let next = dsp::envelope_columns(window, points);
        if self.wave.len() != points {
            self.wave = vec![(0.0, 0.0); points];
        }
        let blend = 1.0 - (-25.0 * dt.clamp(0.0, 0.5)).exp();
        for (w, (lo, hi)) in self.wave.iter_mut().zip(next) {
            let scale = |v: f32| if starved { 0.0 } else { (v * gain).clamp(-1.0, 1.0) };
            w.0 += (scale(lo) - w.0) * blend;
            w.1 += (scale(hi) - w.1) * blend;
        }
        &self.wave
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: u32 = 48_000;

    fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (TAU * freq * i as f32 / SR as f32).sin()).collect()
    }

    /// Feeds `samples` in 1/30 s chunks (phase-continuous), updating after each.
    fn run(samples: &[f32], bands: usize) -> Frame {
        let mut a = Analyzer::new();
        for chunk in samples.chunks(SR as usize / 30) {
            a.push(chunk, 1, SR);
            a.update(1.0 / 30.0, bands, 1.0);
        }
        a.frame().clone()
    }

    fn argmax(v: &[f32]) -> usize {
        v.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0
    }

    fn band_of(freq: f32, bands: usize) -> usize {
        log_bands(bands, MIN_FREQ, MAX_FREQ).iter().position(|b| freq >= b.lo && freq < b.hi).unwrap()
    }

    #[test]
    fn silence_produces_no_bars_and_reports_silence() {
        let mut a = Analyzer::new();
        a.push(&vec![0.0; 8192], 2, SR);
        a.update(1.0 / 30.0, 32, 1.0);
        assert!(a.frame().bands.iter().all(|&b| b == 0.0));
        assert_eq!(a.signal(), Signal::Silence);
    }

    #[test]
    fn no_samples_reports_no_signal() {
        let mut a = Analyzer::new();
        for _ in 0..10 {
            a.update(1.0 / 30.0, 32, 1.0);
        }
        assert_eq!(a.signal(), Signal::None);
        assert!(a.frame().bands.iter().all(|&b| b == 0.0));
    }

    #[test]
    fn sine_peaks_in_its_band() {
        for freq in [100.0, 1_000.0, 8_000.0] {
            let f = run(&sine(freq, 0.05, SR as usize), 32);
            assert_eq!(argmax(&f.bands), band_of(freq, 32), "freq {freq}: {:?}", f.bands);
            assert!(f.bands[band_of(freq, 32)] > 0.3);
        }
    }

    #[test]
    fn two_tones_produce_two_separate_peaks() {
        let a = sine(200.0, 0.05, SR as usize);
        let b = sine(5_000.0, 0.05, SR as usize);
        let mix: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let f = run(&mix, 32);
        let (lo, hi) = (band_of(200.0, 32), band_of(5_000.0, 32));
        let mid = (lo + hi) / 2;
        assert!(f.bands[lo] > 0.3 && f.bands[hi] > 0.3, "{:?}", f.bands);
        assert!(f.bands[mid] < f.bands[lo] * 0.5, "valley between tones: {:?}", f.bands);
    }

    #[test]
    fn dc_offset_does_not_light_up_the_spectrum() {
        let f = run(&vec![0.5; SR as usize], 32);
        // DC sits below MIN_FREQ; window leakage may touch the lowest bands only.
        assert!(f.bands[8..].iter().all(|&b| b < 0.05), "{:?}", f.bands);
    }

    #[test]
    fn short_buffers_are_zero_padded() {
        let mut a = Analyzer::new();
        a.push(&sine(1_000.0, 0.5, 100), 1, SR);
        let f = a.update(1.0 / 30.0, 16, 1.0);
        assert_eq!(f.bands.len(), 16);
        assert!(f.bands.iter().all(|b| b.is_finite() && (0.0..=1.0).contains(b)));
    }

    #[test]
    fn non_finite_input_is_sanitized() {
        let mut a = Analyzer::new();
        a.push(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.5], 2, SR);
        let f = a.update(1.0 / 30.0, 8, 1.0).clone();
        assert!(f.bands.iter().chain(&f.peaks).all(|b| b.is_finite()));
        assert!(f.rms.is_finite() && f.peak.is_finite());
        let w = a.waveform(40, 1.0 / 30.0, 1.0);
        assert!(w.iter().all(|v| v.0.is_finite() && v.1.is_finite()));
    }

    #[test]
    fn stereo_downmix_cancels_opposite_phase() {
        let s = sine(1_000.0, 0.5, 4096);
        let inter: Vec<f32> = s.iter().flat_map(|&x| [x, -x]).collect();
        let mut a = Analyzer::new();
        a.push(&inter, 2, SR);
        a.update(1.0 / 30.0, 16, 1.0);
        assert_eq!(a.signal(), Signal::Silence);
    }

    #[test]
    fn bars_decay_after_signal_stops() {
        let mut a = Analyzer::new();
        a.push(&sine(1_000.0, 0.5, 8192), 1, SR);
        a.update(1.0 / 30.0, 16, 1.0);
        let before = a.frame().bands.iter().cloned().fold(0.0, f32::max);
        for _ in 0..60 {
            a.update(1.0 / 30.0, 16, 1.0);
        }
        let after = a.frame().bands.iter().cloned().fold(0.0, f32::max);
        assert!(before > 0.1 && after < 0.01, "{before} -> {after}");
        assert_eq!(a.signal(), Signal::None);
    }

    #[test]
    fn sensitivity_scales_output() {
        let s = sine(1_000.0, 0.05, 8192);
        let mut lo = Analyzer::new();
        let mut hi = Analyzer::new();
        lo.push(&s, 1, SR);
        hi.push(&s, 1, SR);
        let b = band_of(1_000.0, 16);
        let l = lo.update(0.1, 16, 0.5).bands[b];
        let h = hi.update(0.1, 16, 2.0).bands[b];
        assert!(h > l);
    }

    #[test]
    fn waveform_is_adaptive_and_bounded() {
        let mut a = Analyzer::new();
        a.push(&sine(440.0, 0.05, 8192), 1, SR);
        let mut w = Vec::new();
        for _ in 0..60 {
            a.push(&sine(440.0, 0.05, 1600), 1, SR);
            a.update(1.0 / 30.0, 8, 1.0);
            w = a.waveform(80, 1.0 / 30.0, 1.0).to_vec();
        }
        let peak = w.iter().fold(0.0f32, |m, v| m.max(v.0.abs()).max(v.1.abs()));
        assert!(peak > 0.4 && peak <= 1.0, "quiet signal should be boosted: {peak}");
        assert_eq!(w.len(), 80);
    }

    #[test]
    fn waveform_of_silence_is_flat() {
        let mut a = Analyzer::new();
        a.push(&vec![0.0; 4096], 1, SR);
        let w = a.waveform(20, 1.0 / 30.0, 1.0);
        assert!(w.iter().all(|&v| v == (0.0, 0.0)));
    }
}
