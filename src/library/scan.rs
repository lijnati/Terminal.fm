//! Recursive directory scanning and tag extraction.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use lofty::prelude::*;
use walkdir::WalkDir;

use super::track::{RawTags, Track, is_supported};

/// Progress notifications emitted while scanning.
#[derive(Debug, Clone)]
pub enum ScanProgress {
    Discovered(usize),
    Read { done: usize, total: usize },
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub tracks: Vec<Track>,
    /// Files that looked like audio but could not be read, with a short reason.
    pub failures: Vec<(PathBuf, String)>,
    /// Directories that could not be read at all (missing, permission denied).
    pub unreadable_roots: Vec<(PathBuf, String)>,
}

/// Lists supported audio files under `roots`, sorted and deduplicated.
pub fn discover(roots: &[PathBuf], unreadable: &mut Vec<(PathBuf, String)>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in roots {
        if let Err(e) = std::fs::read_dir(root) {
            unreadable.push((root.clone(), e.to_string()));
            continue;
        }
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
        for entry in walker.flatten() {
            if entry.file_type().is_file() && is_supported(entry.path()) {
                files.push(entry.into_path());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

fn mtime_of(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Reads tags and duration for one file.
pub fn read_track(path: &Path) -> Result<Track, String> {
    let tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let duration = tagged.properties().duration();
    let mut raw = RawTags { duration, ..Default::default() };
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        raw.title = tag.title().map(|s| s.into_owned());
        raw.artist = tag.artist().map(|s| s.into_owned());
        raw.album = tag.album().map(|s| s.into_owned());
        raw.genre = tag.genre().map(|s| s.into_owned());
        raw.track_no = tag.track();
        raw.disc_no = tag.disk();
        raw.year = tag.date().map(|d| d.year);
        raw.album_artist = tag.get_string(ItemKey::AlbumArtist).map(str::to_owned);
    }
    if raw.duration == Duration::ZERO {
        return Err("could not determine duration (file may be empty or corrupt)".into());
    }
    Ok(Track::from_tags(path.to_path_buf(), raw, mtime_of(path)))
}

/// Scans `roots`, reusing entries from `previous` whose modification time is unchanged.
pub fn scan(roots: &[PathBuf], previous: &[Track], mut progress: impl FnMut(ScanProgress)) -> ScanResult {
    let mut result = ScanResult::default();
    let files = discover(roots, &mut result.unreadable_roots);
    progress(ScanProgress::Discovered(files.len()));

    let cached: HashMap<&Path, &Track> = previous.iter().map(|t| (t.path.as_path(), t)).collect();
    let total = files.len();
    for (i, path) in files.into_iter().enumerate() {
        let reused = cached.get(path.as_path()).filter(|t| t.mtime != 0 && t.mtime == mtime_of(&path));
        match reused {
            Some(t) => result.tracks.push((*t).clone()),
            None => match read_track(&path) {
                Ok(t) => result.tracks.push(t),
                Err(e) => result.failures.push((path, e)),
            },
        }
        if (i + 1) % 25 == 0 || i + 1 == total {
            progress(ScanProgress::Read { done: i + 1, total });
        }
    }
    result
}
