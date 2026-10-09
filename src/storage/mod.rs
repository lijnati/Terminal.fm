//! Configuration and persistence. TOML for user-editable config, JSON for app state.

pub mod config;
pub mod paths;
pub mod state;

pub use config::{Config, ConfigLoad, ThemeName, VisualizerMode};
pub use paths::AppPaths;
pub use state::{LibraryCache, Playlist, State, TrackRef};
