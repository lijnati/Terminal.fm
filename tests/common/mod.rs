//! Deterministic audio fixtures for integration tests.
#![allow(dead_code)]

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

/// Writes a 16-bit stereo sine WAV.
pub fn write_sine_wav(path: &Path, freq: f32, secs: f32, sample_rate: u32) -> PathBuf {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let n = (secs * sample_rate as f32) as usize;
    for i in 0..n {
        let s = (0.5 * (TAU * freq * i as f32 / sample_rate as f32).sin() * i16::MAX as f32) as i16;
        w.write_sample(s).unwrap();
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
    path.to_path_buf()
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}
