//! Terminal.fm — a keyboard-first terminal music player with a real-time audio visualizer.
//!
//! Layering: `ui → app → {audio, analysis, library, playback, storage}`.
//! Only `ui` and `app` know about the terminal.

pub mod analysis;
pub mod app;
pub mod audio;
pub mod cli;
pub mod input;
pub mod library;
pub mod playback;
pub mod storage;
pub mod ui;
