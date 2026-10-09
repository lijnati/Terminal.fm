//! The local music library: scanning, grouping, sorting and search.
//!
//! Independent of playback and UI.

pub mod scan;
pub mod search;
pub mod track;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub use track::{Track, format_duration};

use search::{SearchIndex, fold};

/// Index into [`Library::tracks`]. Only valid for the library it came from.
pub type TrackIdx = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Artist,
    Title,
    Album,
    Duration,
    Year,
}

impl SortKey {
    pub const ALL: [SortKey; 5] = [Self::Artist, Self::Title, Self::Album, Self::Duration, Self::Year];
    pub fn label(self) -> &'static str {
        match self {
            Self::Artist => "artist",
            Self::Title => "title",
            Self::Album => "album",
            Self::Duration => "length",
            Self::Year => "year",
        }
    }
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// A named group of tracks (artist, album, genre).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    /// Secondary label: album artist for albums, empty otherwise.
    pub detail: String,
    pub tracks: Vec<TrackIdx>,
}

#[derive(Debug, Default)]
pub struct Library {
    tracks: Vec<Track>,
    by_path: HashMap<PathBuf, TrackIdx>,
    index: SearchIndex,
}

impl Library {
    pub fn new(mut tracks: Vec<Track>) -> Self {
        // Canonical order: artist, album, disc, track number, title.
        tracks.sort_by(|a, b| {
            fold(a.group_artist())
                .cmp(&fold(b.group_artist()))
                .then_with(|| fold(&a.album).cmp(&fold(&b.album)))
                .then(a.disc_no.cmp(&b.disc_no))
                .then(a.track_no.cmp(&b.track_no))
                .then_with(|| fold(&a.title).cmp(&fold(&b.title)))
                .then_with(|| a.path.cmp(&b.path))
        });
        tracks.dedup_by(|a, b| a.path == b.path);
        let by_path = tracks.iter().enumerate().map(|(i, t)| (t.path.clone(), i)).collect();
        let index = SearchIndex::build(&tracks);
        Self { tracks, by_path, index }
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
    pub fn len(&self) -> usize {
        self.tracks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
    pub fn get(&self, idx: TrackIdx) -> Option<&Track> {
        self.tracks.get(idx)
    }
    pub fn find(&self, path: &Path) -> Option<TrackIdx> {
        self.by_path.get(path).copied()
    }

    pub fn search(&self, query: &str) -> Vec<TrackIdx> {
        self.index.search(query)
    }

    /// All track indices ordered by `key` (stable relative to canonical order).
    pub fn sorted(&self, key: SortKey) -> Vec<TrackIdx> {
        let mut v: Vec<TrackIdx> = (0..self.tracks.len()).collect();
        let t = &self.tracks;
        match key {
            SortKey::Artist => {}
            SortKey::Title => v.sort_by_cached_key(|&i| fold(&t[i].title)),
            SortKey::Album => v.sort_by_cached_key(|&i| fold(&t[i].album)),
            SortKey::Duration => v.sort_by_key(|&i| t[i].duration),
            SortKey::Year => v.sort_by_key(|&i| std::cmp::Reverse(t[i].year)),
        }
        v
    }

    fn group_by(&self, key: impl Fn(&Track) -> Option<(String, String)>) -> Vec<Group> {
        let mut groups: BTreeMap<String, Group> = BTreeMap::new();
        for (i, t) in self.tracks.iter().enumerate() {
            let Some((name, detail)) = key(t) else { continue };
            groups
                .entry(fold(&name) + "\u{0}" + &fold(&detail))
                .or_insert_with(|| Group { name, detail, tracks: Vec::new() })
                .tracks
                .push(i);
        }
        groups.into_values().collect()
    }

    pub fn artists(&self) -> Vec<Group> {
        self.group_by(|t| Some((t.artist.clone(), String::new())))
    }
    pub fn albums(&self) -> Vec<Group> {
        self.group_by(|t| Some((t.album.clone(), t.group_artist().to_string())))
    }
    pub fn genres(&self) -> Vec<Group> {
        self.group_by(|t| t.genre.clone().map(|g| (g, String::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use track::RawTags;

    fn t(path: &str, title: &str, artist: &str, album: &str, no: u32, genre: Option<&str>) -> Track {
        Track::from_tags(
            PathBuf::from(path),
            RawTags {
                title: Some(title.into()),
                artist: Some(artist.into()),
                album: Some(album.into()),
                track_no: Some(no),
                genre: genre.map(Into::into),
                duration: Duration::from_secs(100 + no as u64),
                ..Default::default()
            },
            0,
        )
    }

    fn lib() -> Library {
        Library::new(vec![
            t("/b2", "Wait", "M83", "Hurry Up", 2, Some("Synthpop")),
            t("/a1", "Intro", "The xx", "xx", 1, Some("Indie")),
            t("/b1", "Intro", "M83", "Hurry Up", 1, Some("Synthpop")),
            t("/c1", "Digital Love", "Daft Punk", "Discovery", 3, None),
            t("/b1", "Intro", "M83", "Hurry Up", 1, Some("Synthpop")),
        ])
    }

    #[test]
    fn canonical_order_and_dedup() {
        let l = lib();
        let titles: Vec<_> = l.tracks().iter().map(|t| t.path.to_str().unwrap()).collect();
        assert_eq!(titles, vec!["/c1", "/b1", "/b2", "/a1"]);
        assert_eq!(l.find(Path::new("/b2")), Some(2));
    }

    #[test]
    fn groups_albums_artists_genres() {
        let l = lib();
        let albums = l.albums();
        assert_eq!(albums.len(), 3);
        let hurry = albums.iter().find(|g| g.name == "Hurry Up").unwrap();
        assert_eq!(hurry.detail, "M83");
        assert_eq!(hurry.tracks, vec![1, 2]);
        assert_eq!(l.artists().len(), 3);
        // Tracks without a genre are not grouped.
        let genres = l.genres();
        assert_eq!(genres.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["Indie", "Synthpop"]);
    }

    #[test]
    fn sorting() {
        let l = lib();
        let by_title: Vec<_> =
            l.sorted(SortKey::Title).iter().map(|&i| l.tracks()[i].title.clone()).collect();
        assert_eq!(by_title, vec!["Digital Love", "Intro", "Intro", "Wait"]);
        let by_len = l.sorted(SortKey::Duration);
        assert_eq!(l.tracks()[by_len[0]].duration, Duration::from_secs(101));
    }
}
