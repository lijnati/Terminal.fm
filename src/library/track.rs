//! Track model and metadata normalization.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
pub const UNKNOWN_ALBUM: &str = "Unknown Album";

/// File extensions Terminal.fm attempts to play. Each maps to a decoder compiled
/// into rodio/symphonia (see `Cargo.toml` features).
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "ogg", "oga", "m4a", "mp4"];

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUPPORTED_EXTENSIONS.iter().any(|s| s.eq_ignore_ascii_case(e)))
}

/// A playable item in the library. Identity is the absolute path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    #[serde(with = "duration_ms")]
    pub duration: Duration,
    /// Modification time (seconds since epoch) used to skip unchanged files on rescan.
    #[serde(default)]
    pub mtime: u64,
}

/// Raw tag values as read from a file, before fallbacks are applied.
#[derive(Debug, Default, Clone)]
pub struct RawTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub genre: Option<String>,
    pub year: Option<u16>,
    pub duration: Duration,
}

/// Trims whitespace and NUL padding; empty strings become `None`.
pub fn clean(s: Option<String>) -> Option<String> {
    let s = s?;
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// Turns a filename stem like `03 - Midnight_City` into `Midnight City`.
pub fn title_from_path(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let spaced = stem.replace('_', " ");
    // Strip a leading track number such as "03 ", "03 - ", "03. ".
    let trimmed = spaced.trim_start();
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    let rest = if (1..=3).contains(&digits) {
        let after = &trimmed[digits..];
        let after = after.trim_start_matches([' ', '.', '-', '_']);
        if after.is_empty() { trimmed } else { after }
    } else {
        trimmed
    };
    let rest = rest.trim();
    if rest.is_empty() { "Untitled".to_string() } else { rest.to_string() }
}

impl Track {
    /// Builds a track from raw tags, applying filename/folder fallbacks.
    pub fn from_tags(path: PathBuf, raw: RawTags, mtime: u64) -> Self {
        let title = clean(raw.title).unwrap_or_else(|| title_from_path(&path));
        let album_artist = clean(raw.album_artist);
        let artist =
            clean(raw.artist).or_else(|| album_artist.clone()).unwrap_or_else(|| UNKNOWN_ARTIST.to_string());
        let album = clean(raw.album)
            .or_else(|| {
                path.parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .filter(|n| !n.is_empty())
            })
            .unwrap_or_else(|| UNKNOWN_ALBUM.to_string());
        Track {
            path,
            title,
            artist,
            album,
            album_artist,
            track_no: raw.track_no.filter(|n| *n > 0),
            disc_no: raw.disc_no.filter(|n| *n > 0),
            genre: clean(raw.genre),
            year: raw.year.filter(|y| *y > 0),
            duration: raw.duration,
            mtime,
        }
    }

    /// Artist used for album grouping (album artist when present).
    pub fn group_artist(&self) -> &str {
        self.album_artist.as_deref().unwrap_or(&self.artist)
    }
}

mod duration_ms {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        Ok(Duration::from_millis(u64::deserialize(d)?))
    }
}

/// `mm:ss` (or `h:mm:ss` for long tracks).
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_fallback_strips_track_numbers_and_underscores() {
        assert_eq!(title_from_path(Path::new("/m/03 - Midnight_City.mp3")), "Midnight City");
        assert_eq!(title_from_path(Path::new("/m/01. Intro.flac")), "Intro");
        assert_eq!(title_from_path(Path::new("/m/1979.mp3")), "1979");
        assert_eq!(title_from_path(Path::new("/m/42.mp3")), "42");
        assert_eq!(title_from_path(Path::new("/m/Song.mp3")), "Song");
    }

    #[test]
    fn missing_tags_use_fallbacks() {
        let t = Track::from_tags(PathBuf::from("/music/Hurry Up/02_Intro.mp3"), RawTags::default(), 0);
        assert_eq!(t.title, "Intro");
        assert_eq!(t.artist, UNKNOWN_ARTIST);
        assert_eq!(t.album, "Hurry Up");
    }

    #[test]
    fn whitespace_and_nul_tags_are_treated_as_missing() {
        let raw = RawTags {
            title: Some("  \0".into()),
            artist: Some(" M83 ".into()),
            album_artist: None,
            track_no: Some(0),
            year: Some(0),
            ..Default::default()
        };
        let t = Track::from_tags(PathBuf::from("/a/b/x.flac"), raw, 0);
        assert_eq!(t.title, "x");
        assert_eq!(t.artist, "M83");
        assert_eq!(t.track_no, None);
        assert_eq!(t.year, None);
    }

    #[test]
    fn artist_falls_back_to_album_artist() {
        let raw = RawTags { album_artist: Some("Daft Punk".into()), ..Default::default() };
        let t = Track::from_tags(PathBuf::from("/x.mp3"), raw, 0);
        assert_eq!(t.artist, "Daft Punk");
        assert_eq!(t.group_artist(), "Daft Punk");
    }

    #[test]
    fn supported_extensions_are_case_insensitive() {
        assert!(is_supported(Path::new("a.MP3")));
        assert!(is_supported(Path::new("a.flac")));
        assert!(!is_supported(Path::new("a.txt")));
        assert!(!is_supported(Path::new("noext")));
    }

    #[test]
    fn durations_format() {
        assert_eq!(format_duration(Duration::from_secs(84)), "01:24");
        assert_eq!(format_duration(Duration::from_secs(3725)), "1:02:05");
    }
}
