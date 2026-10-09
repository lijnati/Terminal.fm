//! Case- and accent-insensitive ranked search.

use super::track::Track;

/// Lowercases and folds common Latin diacritics so "Beyoncé" matches "beyonce".
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let f = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'a',
            'ç' | 'Ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
            'ñ' | 'Ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
            'ý' | 'ÿ' | 'Ý' => 'y',
            'ß' => {
                out.push_str("ss");
                continue;
            }
            _ => {
                out.extend(c.to_lowercase());
                continue;
            }
        };
        out.push(f);
    }
    out
}

/// Score of `needle` (already folded, non-empty) within `hay` (already folded).
/// 3 = field starts with needle, 2 = a word starts with it, 1 = substring, 0 = none.
fn field_score(hay: &str, needle: &str) -> u32 {
    if hay.starts_with(needle) {
        3
    } else if hay.match_indices(needle).any(|(i, _)| hay[..i].ends_with(|c: char| !c.is_alphanumeric())) {
        2
    } else if hay.contains(needle) {
        1
    } else {
        0
    }
}

/// Precomputed folded fields for fast repeated searching.
#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    entries: Vec<[String; 3]>,
}

impl SearchIndex {
    pub fn build(tracks: &[Track]) -> Self {
        Self { entries: tracks.iter().map(|t| [fold(&t.title), fold(&t.artist), fold(&t.album)]).collect() }
    }

    /// Returns indices into the track list ordered by relevance.
    /// Every whitespace-separated query token must match some field.
    pub fn search(&self, query: &str) -> Vec<usize> {
        let q = fold(query);
        let tokens: Vec<&str> = q.split_whitespace().collect();
        if tokens.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<(u32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, fields)| {
                let mut total = 0;
                for tok in &tokens {
                    // Title matches weigh most, then artist, then album.
                    let best = fields
                        .iter()
                        .zip([3u32, 2, 1])
                        .map(|(f, w)| field_score(f, tok) * w)
                        .max()
                        .unwrap_or(0);
                    if best == 0 {
                        return None;
                    }
                    total += best;
                }
                Some((total, i))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        hits.into_iter().map(|(_, i)| i).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::track::RawTags;
    use std::path::PathBuf;

    fn t(title: &str, artist: &str, album: &str) -> Track {
        Track::from_tags(
            PathBuf::from(format!("/{title}.mp3")),
            RawTags {
                title: Some(title.into()),
                artist: Some(artist.into()),
                album: Some(album.into()),
                ..Default::default()
            },
            0,
        )
    }

    fn lib() -> Vec<Track> {
        vec![
            t("Midnight City", "M83", "Hurry Up, We're Dreaming"),
            t("Intro", "The xx", "xx"),
            t("Something About Us", "Daft Punk", "Discovery"),
            t("Halo", "Beyoncé", "I Am... Sasha Fierce"),
            t("City Lights", "Someone", "After Midnight"),
        ]
    }

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Beyoncé"), "beyonce");
        assert_eq!(fold("STRASSE"), "strasse");
        assert_eq!(fold("Straße"), "strasse");
    }

    #[test]
    fn finds_by_title_artist_and_album() {
        let tracks = lib();
        let idx = SearchIndex::build(&tracks);
        assert_eq!(idx.search("intro"), vec![1]);
        assert_eq!(idx.search("daft"), vec![2]);
        assert_eq!(idx.search("discovery"), vec![2]);
        assert_eq!(idx.search("beyonce"), vec![3]);
    }

    #[test]
    fn ranks_title_prefix_above_album_match() {
        let tracks = lib();
        let idx = SearchIndex::build(&tracks);
        // "midnight": title prefix of #0 vs. album word of #4.
        assert_eq!(idx.search("midnight"), vec![0, 4]);
        // "city": title word of #0 vs. title prefix of #4.
        assert_eq!(idx.search("city"), vec![4, 0]);
    }

    #[test]
    fn all_tokens_must_match() {
        let tracks = lib();
        let idx = SearchIndex::build(&tracks);
        assert_eq!(idx.search("m83 city"), vec![0]);
        assert!(idx.search("m83 halo").is_empty());
    }

    #[test]
    fn empty_query_returns_nothing() {
        let idx = SearchIndex::build(&lib());
        assert!(idx.search("   ").is_empty());
    }
}
