//! Persisted user data: favorites, playlists, recently played (`state.json`),
//! plus the rebuildable library cache.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::paths::write_atomic;
use crate::library::{Library, Track, TrackIdx};
use crate::storage::config::VisualizerMode;

pub const RECENT_LIMIT: usize = 100;
const STATE_VERSION: u32 = 1;

/// A stored reference to a track. The path is the primary key; the fingerprint
/// lets us re-link entries whose file was moved or renamed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackRef {
    pub path: PathBuf,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration_secs: u64,
}

impl TrackRef {
    pub fn of(t: &Track) -> Self {
        Self {
            path: t.path.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_secs: t.duration.as_secs(),
        }
    }

    fn matches(&self, t: &Track) -> bool {
        !self.title.is_empty()
            && self.title == t.title
            && self.artist == t.artist
            && self.album == t.album
            && t.duration.as_secs().abs_diff(self.duration_secs) <= 1
    }

    /// Finds the track in `lib`: exact path first, then by fingerprint.
    /// Updates `self.path` when re-linked.
    pub fn resolve(&mut self, lib: &Library) -> Option<TrackIdx> {
        if let Some(i) = lib.find(&self.path) {
            return Some(i);
        }
        let mut hits = lib.tracks().iter().enumerate().filter(|(_, t)| self.matches(t));
        let (i, t) = hits.next()?;
        if hits.next().is_some() {
            return None; // ambiguous: don't guess
        }
        self.path = t.path.clone();
        Some(i)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<TrackRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub favorites: Vec<TrackRef>,
    pub playlists: Vec<Playlist>,
    /// Most recent first.
    pub recent: Vec<TrackRef>,
    pub volume: Option<u8>,
    pub muted: bool,
    pub visualizer: Option<VisualizerMode>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            favorites: Vec::new(),
            playlists: Vec::new(),
            recent: Vec::new(),
            volume: None,
            muted: false,
            visualizer: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistError {
    EmptyName,
    DuplicateName,
    NotFound,
}

impl std::fmt::Display for PlaylistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::EmptyName => "playlist name cannot be empty",
            Self::DuplicateName => "a playlist with that name already exists",
            Self::NotFound => "playlist not found",
        })
    }
}

impl State {
    /// Loads state; a corrupt file is moved aside (`state.json.bak`) and defaults returned.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        match std::fs::read_to_string(path) {
            Err(_) => (Self::default(), None),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => (s, None),
                Err(e) => {
                    let bak = path.with_extension("json.bak");
                    let _ = std::fs::rename(path, &bak);
                    (
                        Self::default(),
                        Some(format!("state file was unreadable ({e}); saved a copy to {}", bak.display())),
                    )
                }
            },
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        write_atomic(path, &json)
    }

    /// Re-links moved files against a freshly loaded library. Returns true if anything changed.
    pub fn relink(&mut self, lib: &Library) -> bool {
        let mut changed = false;
        let all = self
            .favorites
            .iter_mut()
            .chain(self.recent.iter_mut())
            .chain(self.playlists.iter_mut().flat_map(|p| p.tracks.iter_mut()));
        for r in all {
            let before = r.path.clone();
            r.resolve(lib);
            changed |= r.path != before;
        }
        changed
    }

    pub fn is_favorite(&self, path: &Path) -> bool {
        self.favorites.iter().any(|r| r.path == path)
    }

    /// Toggles favorite status, returning the new status.
    pub fn toggle_favorite(&mut self, t: &Track) -> bool {
        if let Some(i) = self.favorites.iter().position(|r| r.path == t.path) {
            self.favorites.remove(i);
            false
        } else {
            self.favorites.push(TrackRef::of(t));
            true
        }
    }

    pub fn push_recent(&mut self, t: &Track) {
        self.recent.retain(|r| r.path != t.path);
        self.recent.insert(0, TrackRef::of(t));
        self.recent.truncate(RECENT_LIMIT);
    }

    fn name_taken(&self, name: &str, except: Option<usize>) -> bool {
        self.playlists.iter().enumerate().any(|(i, p)| Some(i) != except && p.name.eq_ignore_ascii_case(name))
    }

    pub fn create_playlist(&mut self, name: &str) -> Result<usize, PlaylistError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PlaylistError::EmptyName);
        }
        if self.name_taken(name, None) {
            return Err(PlaylistError::DuplicateName);
        }
        self.playlists.push(Playlist { name: name.to_string(), tracks: Vec::new() });
        Ok(self.playlists.len() - 1)
    }

    pub fn rename_playlist(&mut self, idx: usize, name: &str) -> Result<(), PlaylistError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(PlaylistError::EmptyName);
        }
        if idx >= self.playlists.len() {
            return Err(PlaylistError::NotFound);
        }
        if self.name_taken(name, Some(idx)) {
            return Err(PlaylistError::DuplicateName);
        }
        self.playlists[idx].name = name.to_string();
        Ok(())
    }

    pub fn delete_playlist(&mut self, idx: usize) -> Result<Playlist, PlaylistError> {
        if idx >= self.playlists.len() {
            return Err(PlaylistError::NotFound);
        }
        Ok(self.playlists.remove(idx))
    }

    /// Adds a track; returns false if it was already present.
    pub fn add_to_playlist(&mut self, idx: usize, t: &Track) -> Result<bool, PlaylistError> {
        let p = self.playlists.get_mut(idx).ok_or(PlaylistError::NotFound)?;
        if p.tracks.iter().any(|r| r.path == t.path) {
            return Ok(false);
        }
        p.tracks.push(TrackRef::of(t));
        Ok(true)
    }

    pub fn remove_from_playlist(&mut self, idx: usize, pos: usize) -> Result<(), PlaylistError> {
        let p = self.playlists.get_mut(idx).ok_or(PlaylistError::NotFound)?;
        if pos >= p.tracks.len() {
            return Err(PlaylistError::NotFound);
        }
        p.tracks.remove(pos);
        Ok(())
    }

    /// Moves an entry up (-1) or down (+1) within a playlist. Returns the new position.
    pub fn move_in_playlist(&mut self, idx: usize, pos: usize, delta: isize) -> Option<usize> {
        let p = self.playlists.get_mut(idx)?;
        let to = pos.checked_add_signed(delta)?;
        if pos >= p.tracks.len() || to >= p.tracks.len() {
            return None;
        }
        p.tracks.swap(pos, to);
        Some(to)
    }
}

/// The scanned library, cached so startup does not rescan.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct LibraryCache {
    pub roots: Vec<PathBuf>,
    pub tracks: Vec<Track>,
}

impl LibraryCache {
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read(path).ok()?;
        serde_json::from_slice(&text).ok()
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec(self).map_err(std::io::Error::other)?;
        write_atomic(path, &json)
    }
}

/// Seconds rendered as a duration, for TrackRef display when the file is missing.
pub fn ref_duration(r: &TrackRef) -> Duration {
    Duration::from_secs(r.duration_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::track::RawTags;

    fn track(path: &str, title: &str) -> Track {
        Track::from_tags(
            PathBuf::from(path),
            RawTags {
                title: Some(title.into()),
                artist: Some("A".into()),
                album: Some("B".into()),
                duration: Duration::from_secs(200),
                ..Default::default()
            },
            0,
        )
    }

    #[test]
    fn playlist_lifecycle() {
        let mut s = State::default();
        let t1 = track("/1.mp3", "One");
        let t2 = track("/2.mp3", "Two");
        assert_eq!(s.create_playlist("  "), Err(PlaylistError::EmptyName));
        let p = s.create_playlist("Focus").unwrap();
        assert_eq!(s.create_playlist("focus"), Err(PlaylistError::DuplicateName));
        assert_eq!(s.add_to_playlist(p, &t1), Ok(true));
        assert_eq!(s.add_to_playlist(p, &t1), Ok(false));
        assert_eq!(s.add_to_playlist(p, &t2), Ok(true));
        assert_eq!(s.move_in_playlist(p, 1, -1), Some(0));
        assert_eq!(s.playlists[p].tracks[0].title, "Two");
        assert_eq!(s.move_in_playlist(p, 0, -1), None);
        s.rename_playlist(p, "Deep Focus").unwrap();
        assert_eq!(s.playlists[p].name, "Deep Focus");
        s.remove_from_playlist(p, 0).unwrap();
        assert_eq!(s.playlists[p].tracks.len(), 1);
        assert_eq!(s.delete_playlist(p).unwrap().name, "Deep Focus");
        assert_eq!(s.delete_playlist(0), Err(PlaylistError::NotFound));
    }

    #[test]
    fn favorites_toggle() {
        let mut s = State::default();
        let t = track("/1.mp3", "One");
        assert!(s.toggle_favorite(&t));
        assert!(s.is_favorite(&t.path));
        assert!(!s.toggle_favorite(&t));
        assert!(!s.is_favorite(&t.path));
    }

    #[test]
    fn recent_is_deduplicated_and_bounded() {
        let mut s = State::default();
        for i in 0..150 {
            s.push_recent(&track(&format!("/{i}.mp3"), "x"));
        }
        s.push_recent(&track("/120.mp3", "x"));
        assert_eq!(s.recent.len(), RECENT_LIMIT);
        assert_eq!(s.recent[0].path, PathBuf::from("/120.mp3"));
        assert_eq!(s.recent.iter().filter(|r| r.path == Path::new("/120.mp3")).count(), 1);
    }

    #[test]
    fn relinks_moved_files_by_fingerprint() {
        let mut s = State::default();
        s.toggle_favorite(&track("/old/1.mp3", "One"));
        let lib = Library::new(vec![track("/new/1.mp3", "One"), track("/new/2.mp3", "Two")]);
        assert!(s.relink(&lib));
        assert_eq!(s.favorites[0].path, PathBuf::from("/new/1.mp3"));
    }

    #[test]
    fn ambiguous_relink_is_left_alone() {
        let mut s = State::default();
        s.toggle_favorite(&track("/old/1.mp3", "One"));
        let lib = Library::new(vec![track("/a/1.mp3", "One"), track("/b/1.mp3", "One")]);
        assert!(!s.relink(&lib));
        assert_eq!(s.favorites[0].path, PathBuf::from("/old/1.mp3"));
    }

    #[test]
    fn save_load_round_trip_and_corrupt_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("state.json");
        let mut s = State::default();
        s.create_playlist("Mix").unwrap();
        s.toggle_favorite(&track("/1.mp3", "One"));
        s.volume = Some(40);
        s.save(&p).unwrap();
        let (back, err) = State::load(&p);
        assert!(err.is_none());
        assert_eq!(back, s);

        std::fs::write(&p, "{not json").unwrap();
        let (fresh, err) = State::load(&p);
        assert!(err.is_some());
        assert_eq!(fresh, State::default());
        assert!(dir.path().join("state.json.bak").exists());
    }
}
