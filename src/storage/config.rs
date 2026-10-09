//! User configuration (`config.toml`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::paths::write_atomic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    Mono,
    #[default]
    Midnight,
    Amber,
    Paper,
}

impl ThemeName {
    pub const ALL: [ThemeName; 4] = [Self::Mono, Self::Midnight, Self::Amber, Self::Paper];
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Mono => "Mono",
            Self::Midnight => "Midnight",
            Self::Amber => "Amber",
            Self::Paper => "Paper",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisualizerMode {
    #[default]
    Spectrum,
    Waveform,
    Minimal,
    Off,
}

impl VisualizerMode {
    pub const ALL: [VisualizerMode; 4] = [Self::Spectrum, Self::Waveform, Self::Minimal, Self::Off];
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Spectrum => "SPECTRUM",
            Self::Waveform => "WAVEFORM",
            Self::Minimal => "MINIMAL",
            Self::Off => "OFF",
        }
    }
}

pub const FPS_MIN: u32 = 10;
pub const FPS_MAX: u32 = 60;
pub const SENSITIVITY_MIN: f32 = 0.25;
pub const SENSITIVITY_MAX: f32 = 4.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VisualizerConfig {
    pub mode: VisualizerMode,
    /// Gain applied to analysis output, 0.25–4.0.
    pub sensitivity: f32,
    /// Redraw rate while playing, 10–60.
    pub fps: u32,
    /// Spectrum bar count; 0 = fit to width.
    pub bars: u16,
    /// Show falling peak markers on the spectrum.
    pub peaks: bool,
}

impl Default for VisualizerConfig {
    fn default() -> Self {
        Self { mode: VisualizerMode::Spectrum, sensitivity: 1.0, fps: 30, bars: 0, peaks: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub music_dirs: Vec<PathBuf>,
    pub theme: ThemeName,
    /// Startup volume, 0–100.
    pub volume: u8,
    /// Seek step in seconds for h/l.
    pub seek_seconds: u64,
    pub visualizer: VisualizerConfig,
    /// Key remaps: action name → key (e.g. `play_pause = "space"`).
    pub keys: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            music_dirs: Vec::new(),
            theme: ThemeName::default(),
            volume: 75,
            seek_seconds: 5,
            visualizer: VisualizerConfig::default(),
            keys: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigLoad {
    /// No config file exists yet (first launch).
    Missing,
    Loaded(Config),
    /// The file exists but is invalid; defaults should be used and the file left untouched.
    Invalid(String),
}

impl Config {
    /// Clamps values into their valid ranges.
    pub fn sanitized(mut self) -> Self {
        self.volume = self.volume.min(100);
        self.seek_seconds = self.seek_seconds.clamp(1, 120);
        let v = &mut self.visualizer;
        v.fps = v.fps.clamp(FPS_MIN, FPS_MAX);
        v.sensitivity = if v.sensitivity.is_finite() {
            v.sensitivity.clamp(SENSITIVITY_MIN, SENSITIVITY_MAX)
        } else {
            1.0
        };
        v.bars = v.bars.min(256);
        self.music_dirs.dedup();
        self
    }

    /// Parses config text. Errors are condensed to one line, e.g.
    /// `line 1: unknown variant `neon`, expected one of …`.
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str::<Config>(text).map(Config::sanitized).map_err(|e| {
            let line = e.span().map(|s| text[..s.start.min(text.len())].lines().count().max(1));
            let msg = e.message().trim().to_string();
            match line {
                Some(n) => format!("line {n}: {msg}"),
                None => msg,
            }
        })
    }

    pub fn load(path: &Path) -> ConfigLoad {
        match std::fs::read_to_string(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ConfigLoad::Missing,
            Err(e) => ConfigLoad::Invalid(format!("cannot read {}: {e}", path.display())),
            Ok(text) => match Self::parse(&text) {
                Ok(c) => ConfigLoad::Loaded(c),
                Err(e) => ConfigLoad::Invalid(format!("config.toml {e}")),
            },
        }
    }

    pub fn to_toml(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_default();
        format!("# Terminal.fm configuration. See config.example.toml for all options.\n\n{body}")
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        write_atomic(path, self.to_toml().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut c = Config {
            music_dirs: vec![PathBuf::from("/music"), PathBuf::from("/more music")],
            theme: ThemeName::Amber,
            ..Default::default()
        };
        c.visualizer.mode = VisualizerMode::Waveform;
        c.keys.insert("play_pause".into(), "p".into());
        let back = Config::parse(&c.to_toml()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn partial_files_use_defaults() {
        let c = Config::parse("theme = \"paper\"\n").unwrap();
        assert_eq!(c.theme, ThemeName::Paper);
        assert_eq!(c.volume, 75);
        assert_eq!(c.visualizer.fps, 30);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let c = Config::parse("volume = 250\n[visualizer]\nfps = 500\nsensitivity = 0.0\n").unwrap();
        assert_eq!(c.volume, 100);
        assert_eq!(c.visualizer.fps, FPS_MAX);
        assert_eq!(c.visualizer.sensitivity, SENSITIVITY_MIN);
    }

    #[test]
    fn errors_are_one_line_with_a_line_number() {
        let e = Config::parse("volume = 50\ntheme = \"neon\"\n").unwrap_err();
        assert!(e.starts_with("line 2: "), "{e}");
        assert!(e.contains("neon") && !e.contains('\n'), "{e}");
    }

    #[test]
    fn invalid_files_report_errors() {
        assert!(Config::parse("theme = \"neon\"").is_err());
        assert!(Config::parse("volume = \"loud\"").is_err());
        assert!(Config::parse("unknown_key = 1").is_err());
        assert!(Config::parse("[[[").is_err());
    }

    #[test]
    fn load_distinguishes_missing_and_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        assert!(matches!(Config::load(&p), ConfigLoad::Missing));
        std::fs::write(&p, "volume = [").unwrap();
        assert!(matches!(Config::load(&p), ConfigLoad::Invalid(_)));
        Config::default().save(&p).unwrap();
        assert!(matches!(Config::load(&p), ConfigLoad::Loaded(_)));
    }

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../config.example.toml");
        Config::parse(text).expect("config.example.toml must stay valid");
    }

    #[test]
    fn cycles() {
        assert_eq!(ThemeName::Paper.next(), ThemeName::Mono);
        assert_eq!(VisualizerMode::Off.next(), VisualizerMode::Spectrum);
    }
}
