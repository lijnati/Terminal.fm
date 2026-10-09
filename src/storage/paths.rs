//! Platform-appropriate file locations.

use std::path::{Path, PathBuf};

/// Overrides every location with a single directory (useful for tests and portable installs).
pub const HOME_ENV: &str = "TERMINALFM_HOME";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_file: PathBuf,
    pub state_file: PathBuf,
    pub library_cache: PathBuf,
    pub log_file: PathBuf,
}

impl AppPaths {
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            config_file: dir.join("config.toml"),
            state_file: dir.join("state.json"),
            library_cache: dir.join("library.json"),
            log_file: dir.join("terminalfm.log"),
        }
    }

    /// Resolves locations: `$TERMINALFM_HOME`, else the OS conventions
    /// (XDG on Linux, `~/Library/Application Support` on macOS, `%APPDATA%` on Windows).
    pub fn resolve() -> Option<Self> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            return Some(Self::in_dir(Path::new(&home)));
        }
        let dirs = directories::ProjectDirs::from("", "", "terminalfm")?;
        Some(Self {
            config_file: dirs.config_dir().join("config.toml"),
            state_file: dirs.data_dir().join("state.json"),
            library_cache: dirs.cache_dir().join("library.json"),
            log_file: dirs.cache_dir().join("terminalfm.log"),
        })
    }
}

/// The OS "Music" folder, used as the default suggestion on first launch.
pub fn default_music_dir() -> Option<PathBuf> {
    directories::UserDirs::new().and_then(|u| u.audio_dir().map(Path::to_path_buf))
}

/// Writes atomically: write to a sibling temp file, then rename over the target.
pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// Expands a leading `~` to the home directory.
pub fn expand_tilde(input: &str) -> PathBuf {
    let home = || directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf());
    if input == "~" {
        if let Some(h) = home() {
            return h;
        }
    } else if let Some(rest) = input.strip_prefix("~/").or_else(|| input.strip_prefix("~\\")) {
        if let Some(h) = home() {
            return h.join(rest);
        }
    }
    PathBuf::from(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_parents_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b/c.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        assert!(!dir.path().join("a/b/c.json.tmp").exists());
    }

    #[test]
    fn tilde_expansion_leaves_plain_paths() {
        assert_eq!(expand_tilde("/music"), PathBuf::from("/music"));
        assert_ne!(expand_tilde("~/Music"), PathBuf::from("~/Music"));
    }
}
