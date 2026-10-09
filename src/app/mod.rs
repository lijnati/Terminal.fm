//! Application state and behavior. Turns key presses and engine events into
//! state changes and audio commands. Contains no rendering code.

pub mod list;
pub mod run;
pub mod scan;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::analysis::Analyzer;
use crate::audio::{AudioEngine, Command, EngineOptions, Event, OutputKind, PlayState, TapReader};
use crate::input::{Action, Keymap, TextInput, TextOutcome};
use crate::library::{Group, Library, SortKey, Track, TrackIdx};
use crate::playback::{Advance, Queue, Rng};
use crate::storage::config::{FPS_MAX, FPS_MIN, SENSITIVITY_MAX, SENSITIVITY_MIN};
use crate::storage::paths::expand_tilde;
use crate::storage::{AppPaths, Config, LibraryCache, State, ThemeName, TrackRef, VisualizerMode};
use crate::ui::theme::{ColorDepth, Theme};

pub use list::ListCursor;
use scan::ScanJob;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    NowPlaying,
    Library,
    Playlists,
    Favorites,
    Recent,
    Settings,
}

impl Screen {
    pub const ALL: [Screen; 6] =
        [Self::NowPlaying, Self::Library, Self::Playlists, Self::Favorites, Self::Recent, Self::Settings];
    pub fn label(self) -> &'static str {
        match self {
            Self::NowPlaying => "now playing",
            Self::Library => "library",
            Self::Playlists => "playlists",
            Self::Favorites => "favorites",
            Self::Recent => "recent",
            Self::Settings => "settings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibTab {
    Tracks,
    Artists,
    Albums,
    Genres,
}

impl LibTab {
    pub const ALL: [LibTab; 4] = [Self::Tracks, Self::Artists, Self::Albums, Self::Genres];
    pub fn label(self) -> &'static str {
        match self {
            Self::Tracks => "TRACKS",
            Self::Artists => "ARTISTS",
            Self::Albums => "ALBUMS",
            Self::Genres => "GENRES",
        }
    }
}

/// Library browser state. `groups` and `items` are caches rebuilt by `refresh`.
#[derive(Debug, Default)]
pub struct LibraryView {
    pub tab_index: usize,
    pub sort: SortKey,
    pub groups: Vec<Group>,
    /// Open group (index into `groups`) when drilled in.
    pub open: Option<usize>,
    /// Track list for the Tracks tab or an open group.
    pub tracks: Vec<TrackIdx>,
    pub cursor: ListCursor,
    /// Cursor of the group list, restored when leaving a group.
    pub group_cursor: ListCursor,
}

impl LibraryView {
    pub fn tab(&self) -> LibTab {
        LibTab::ALL[self.tab_index % LibTab::ALL.len()]
    }
    pub fn showing_groups(&self) -> bool {
        self.tab() != LibTab::Tracks && self.open.is_none()
    }
    pub fn len(&self) -> usize {
        if self.showing_groups() { self.groups.len() } else { self.tracks.len() }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Default)]
pub struct PlaylistsView {
    pub cursor: ListCursor,
    /// Index of the open playlist.
    pub open: Option<usize>,
    pub track_cursor: ListCursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    Theme,
    Visualizer,
    Sensitivity,
    Fps,
    Peaks,
    Volume,
    SeekStep,
    Folder(usize),
    AddFolder,
    Rescan,
}

#[derive(Debug)]
pub struct SearchState {
    pub input: TextInput,
    pub results: Vec<TrackIdx>,
    pub cursor: ListCursor,
    /// Keys go to the results list instead of the text field.
    pub results_focused: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptPurpose {
    NewPlaylist { then_add: Vec<PathBuf> },
    RenamePlaylist(usize),
    AddFolder,
}

#[derive(Debug)]
pub struct Prompt {
    pub title: &'static str,
    pub input: TextInput,
    pub purpose: PromptPurpose,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    DeletePlaylist(usize),
    RemoveFolder(usize),
}

#[derive(Debug)]
pub enum Overlay {
    Help {
        scroll: usize,
    },
    Search(SearchState),
    Prompt(Prompt),
    /// Choose a playlist for `tracks`; row 0 is "new playlist".
    PlaylistPicker {
        cursor: ListCursor,
        tracks: Vec<PathBuf>,
    },
    Confirm {
        message: String,
        action: ConfirmAction,
    },
}

#[derive(Debug)]
pub struct Welcome {
    pub input: TextInput,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum Phase {
    Welcome(Welcome),
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub text: String,
    pub kind: StatusKind,
    pub until: Instant,
}

#[derive(Debug, Clone)]
pub struct NowPlaying {
    pub idx: TrackIdx,
    pub duration: Duration,
}

/// Visualizer analysis state shared with the renderer.
pub struct Viz {
    pub analyzer: Analyzer,
    pub buf: Vec<f32>,
    pub last: Instant,
    /// Seconds since the previous analysis update.
    pub dt: f32,
    /// Band count requested by the last render.
    pub bands: usize,
}

pub struct App {
    pub config: Config,
    /// False when the config file was invalid: settings changes are not written back.
    pub config_writable: bool,
    pub paths: Option<AppPaths>,
    pub state: State,
    pub library: Library,
    pub keymap: Keymap,
    pub theme: Theme,
    pub color_depth: ColorDepth,
    pub phase: Phase,
    pub screen: Screen,
    pub overlay: Option<Overlay>,
    pub lib: LibraryView,
    pub playlists: PlaylistsView,
    pub favorites_cursor: ListCursor,
    pub recent_cursor: ListCursor,
    pub settings_cursor: ListCursor,
    pub queue_cursor: ListCursor,
    pub queue_focused: bool,
    pub queue: Queue<TrackIdx>,
    pub now: Option<NowPlaying>,
    pub engine: AudioEngine,
    pub tap: TapReader,
    pub viz: Viz,
    pub visualizer: VisualizerMode,
    pub volume: u8,
    pub muted: bool,
    pub status: Option<Status>,
    pub scan: Option<ScanJob>,
    pub fail_streak: usize,
    pub state_dirty: bool,
    pub last_save: Instant,
    pub quit: bool,
}

/// Everything needed to construct an [`App`].
pub struct AppInit {
    pub config: Config,
    pub config_writable: bool,
    pub first_run: bool,
    pub paths: Option<AppPaths>,
    pub state: State,
    pub library: Library,
    pub engine: EngineOptions,
    pub color_depth: ColorDepth,
    pub warnings: Vec<String>,
}

const STATUS_TTL: Duration = Duration::from_secs(4);
const VOLUME_STEP: u8 = 5;

impl App {
    pub fn new(init: AppInit) -> Self {
        let (engine, tap) = AudioEngine::start(init.engine);
        let (keymap, key_problems) = Keymap::with_overrides(&init.config.keys);
        let volume = init.state.volume.unwrap_or(init.config.volume).min(100);
        let visualizer = init.state.visualizer.unwrap_or(init.config.visualizer.mode);
        let phase = if init.first_run {
            let suggestion = init
                .config
                .music_dirs
                .first()
                .cloned()
                .or_else(crate::storage::paths::default_music_dir)
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            Phase::Welcome(Welcome { input: TextInput::new(suggestion), error: None })
        } else {
            Phase::Main
        };
        let mut app = App {
            theme: Theme::new(init.config.theme, init.color_depth),
            color_depth: init.color_depth,
            config_writable: init.config_writable,
            paths: init.paths,
            muted: init.state.muted,
            state: init.state,
            library: init.library,
            keymap,
            phase,
            screen: Screen::NowPlaying,
            overlay: None,
            lib: LibraryView::default(),
            playlists: PlaylistsView::default(),
            favorites_cursor: ListCursor::default(),
            recent_cursor: ListCursor::default(),
            settings_cursor: ListCursor::default(),
            queue_cursor: ListCursor::default(),
            queue_focused: false,
            queue: Queue::new(Rng::from_time()),
            now: None,
            engine,
            tap,
            viz: Viz { analyzer: Analyzer::new(), buf: Vec::new(), last: Instant::now(), dt: 0.0, bands: 32 },
            visualizer,
            volume,
            status: None,
            scan: None,
            fail_streak: 0,
            state_dirty: false,
            last_save: Instant::now(),
            quit: false,
            config: init.config,
        };
        app.apply_volume();
        app.refresh_library_view();
        if let OutputKind::Null(reason) = app.engine.output().clone() {
            if !reason.contains("--no-audio") {
                app.set_status(
                    StatusKind::Warn,
                    format!("No audio device found — playing silently ({reason})"),
                );
            }
        }
        for w in
            init.warnings.into_iter().chain(key_problems.into_iter().map(|p| format!("config [keys]: {p}")))
        {
            app.set_status(StatusKind::Warn, w);
        }
        app
    }

    // ───────────────────────────── status & persistence ─────────────────────────────

    pub fn set_status(&mut self, kind: StatusKind, text: impl Into<String>) {
        let text = text.into();
        match kind {
            StatusKind::Error => tracing::error!("{text}"),
            StatusKind::Warn => tracing::warn!("{text}"),
            StatusKind::Info => tracing::info!("{text}"),
        }
        let ttl = if kind == StatusKind::Info { STATUS_TTL } else { STATUS_TTL * 2 };
        self.status = Some(Status { text, kind, until: Instant::now() + ttl });
    }

    fn info(&mut self, text: impl Into<String>) {
        self.set_status(StatusKind::Info, text);
    }

    pub fn save_state_now(&mut self) {
        self.state.volume = Some(self.volume);
        self.state.muted = self.muted;
        self.state.visualizer = Some(self.visualizer);
        if let Some(p) = &self.paths {
            if let Err(e) = self.state.save(&p.state_file) {
                tracing::error!("saving state: {e}");
            }
        }
        self.state_dirty = false;
        self.last_save = Instant::now();
    }

    fn mark_dirty(&mut self) {
        self.state_dirty = true;
    }

    fn save_config(&mut self) {
        if !self.config_writable {
            self.set_status(StatusKind::Warn, "config.toml has errors — fix it to save settings");
            return;
        }
        if let Some(p) = &self.paths {
            if let Err(e) = self.config.save(&p.config_file) {
                self.set_status(StatusKind::Error, format!("Couldn't save config: {e}"));
            }
        }
    }

    /// Periodic housekeeping: expire status, debounce state saves, poll scans and the engine.
    pub fn tick(&mut self) {
        if self.status.as_ref().is_some_and(|s| Instant::now() >= s.until) {
            self.status = None;
        }
        if self.state_dirty && self.last_save.elapsed() > Duration::from_secs(2) {
            self.save_state_now();
        }
        while let Some(ev) = self.engine.poll_event() {
            self.on_engine_event(ev);
        }
        if let Some(job) = &mut self.scan {
            if let Some(result) = job.poll() {
                self.scan = None;
                self.finish_scan(result);
                self.leave_welcome_if_ready();
            }
        }
        let (ch, sr) = self.tap.drain(&mut self.viz.buf);
        if !self.viz.buf.is_empty() {
            self.viz.analyzer.push(&self.viz.buf, ch, sr);
        }
    }

    /// Advances visualizer analysis; called once per rendered frame.
    pub fn update_visualizer(&mut self) {
        let now = Instant::now();
        self.viz.dt = (now - self.viz.last).as_secs_f32();
        self.viz.last = now;
        let bands = self.viz.bands.max(1);
        self.viz.analyzer.update(self.viz.dt, bands, self.config.visualizer.sensitivity);
    }

    /// True while visualizer output is still decaying toward rest (e.g. just after pausing).
    pub fn viz_settling(&self) -> bool {
        let f = self.viz.analyzer.frame();
        self.visualizer != VisualizerMode::Off
            && (f.rms > 0.01 || f.bands.iter().chain(&f.peaks).any(|&b| b > 0.01))
    }

    pub fn play_state(&self) -> PlayState {
        self.engine.state()
    }

    /// Whether the screen should animate at full frame rate.
    pub fn animating(&self) -> bool {
        self.visualizer != VisualizerMode::Off
            && self.play_state() == PlayState::Playing
            && matches!(self.phase, Phase::Main)
    }

    // ───────────────────────────── library ─────────────────────────────

    pub fn start_scan(&mut self, foreground: bool) {
        if self.scan.is_some() {
            return;
        }
        if self.config.music_dirs.is_empty() {
            self.set_status(StatusKind::Warn, "No music folders configured — add one in Settings (6)");
            return;
        }
        let previous = self.library.tracks().to_vec();
        self.scan = Some(ScanJob::start(self.config.music_dirs.clone(), previous, foreground));
    }

    fn finish_scan(&mut self, result: crate::library::scan::ScanResult) {
        let old_paths: Vec<PathBuf> = self.library.tracks().iter().map(|t| t.path.clone()).collect();
        let current_path = self.now.as_ref().map(|n| old_paths[n.idx].clone());
        let count = result.tracks.len();
        self.library = Library::new(result.tracks);
        // Remap the queue and the playing track onto the new indices.
        let lib = &self.library;
        self.queue.remap(|&i| old_paths.get(i).and_then(|p| lib.find(p)));
        if let Some(path) = current_path {
            match self.library.find(&path) {
                Some(idx) => {
                    if let Some(n) = &mut self.now {
                        n.idx = idx;
                    }
                }
                None => self.now = None,
            }
        }
        if self.state.relink(&self.library) {
            self.mark_dirty();
        }
        if let Some(p) = &self.paths {
            let cache = LibraryCache {
                roots: self.config.music_dirs.clone(),
                tracks: self.library.tracks().to_vec(),
            };
            if let Err(e) = cache.save(&p.library_cache) {
                tracing::error!("saving library cache: {e}");
            }
        }
        self.refresh_library_view();
        for (root, err) in &result.unreadable_roots {
            tracing::warn!("cannot read {}: {err}", root.display());
        }
        for (path, err) in &result.failures {
            tracing::warn!("skipped {}: {err}", path.display());
        }
        if let Some((root, err)) = result.unreadable_roots.first() {
            self.set_status(StatusKind::Warn, format!("Can't read {}: {err}", root.display()));
        } else if result.failures.is_empty() {
            self.info(format!("Library ready · {count} tracks"));
        } else {
            self.set_status(
                StatusKind::Warn,
                format!("Library ready · {count} tracks · {} skipped (see log)", result.failures.len()),
            );
        }
    }

    pub fn refresh_library_view(&mut self) {
        let v = &mut self.lib;
        v.groups = match v.tab() {
            LibTab::Tracks => Vec::new(),
            LibTab::Artists => self.library.artists(),
            LibTab::Albums => self.library.albums(),
            LibTab::Genres => self.library.genres(),
        };
        if v.open.is_some_and(|g| g >= v.groups.len()) {
            v.open = None;
        }
        v.tracks = match (v.tab(), v.open) {
            (LibTab::Tracks, _) => self.library.sorted(v.sort),
            (_, Some(g)) => v.groups[g].tracks.clone(),
            (_, None) => Vec::new(),
        };
        let len = v.len();
        v.cursor.clamp(len);
        v.group_cursor.clamp(v.groups.len());
    }

    pub fn track(&self, idx: TrackIdx) -> Option<&Track> {
        self.library.get(idx)
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.now.as_ref().and_then(|n| self.library.get(n.idx))
    }

    // ───────────────────────────── playback ─────────────────────────────

    fn apply_volume(&mut self) {
        let v = if self.muted { 0.0 } else { self.volume as f32 / 100.0 };
        // Squared curve approximates perceived loudness.
        self.engine.send(Command::Volume(v * v));
    }

    /// Replaces the queue with `items` and starts playing at `start`.
    pub fn play_list(&mut self, items: Vec<TrackIdx>, start: usize) {
        if items.is_empty() {
            return;
        }
        self.fail_streak = 0;
        self.queue.replace(items, start);
        self.queue_cursor = ListCursor::default();
        self.load_current();
    }

    fn load_current(&mut self) {
        let Some(&idx) = self.queue.current() else {
            self.stop();
            return;
        };
        let Some(track) = self.library.get(idx) else { return };
        let path = track.path.clone();
        let duration = track.duration;
        let t = track.clone();
        self.engine.send(Command::Load { path, paused: false });
        self.now = Some(NowPlaying { idx, duration });
        self.viz.analyzer.reset();
        self.state.push_recent(&t);
        self.mark_dirty();
    }

    pub fn stop(&mut self) {
        self.engine.send(Command::Stop);
        self.now = None;
    }

    fn advance(&mut self, why: Advance) {
        if self.queue.next(why).is_some() {
            self.load_current();
        } else if why == Advance::Auto {
            self.now = None;
            self.info("End of queue");
        } else {
            self.info("End of queue — press r to repeat");
        }
    }

    fn previous(&mut self) {
        if self.engine.position() > Duration::from_secs(3) {
            self.engine.send(Command::Seek(Duration::ZERO));
        } else if self.queue.prev().is_some() {
            self.load_current();
        } else if self.now.is_some() {
            self.engine.send(Command::Seek(Duration::ZERO));
        }
    }

    fn play_pause(&mut self) {
        match self.play_state() {
            PlayState::Playing => {
                self.engine.send(Command::Pause);
            }
            PlayState::Paused => {
                self.engine.send(Command::Play);
            }
            PlayState::Stopped => {
                if self.queue.current().is_some() {
                    self.load_current();
                } else if !self.queue.is_empty() {
                    let items = self.queue.items().to_vec();
                    self.play_list(items, 0);
                } else if self.screen == Screen::Library && !self.lib.showing_groups() && !self.lib.is_empty()
                {
                    self.play_list(self.lib.tracks.clone(), self.lib.cursor.selected);
                } else {
                    self.info("Nothing queued — pick a track and press Enter");
                }
            }
        }
    }

    fn seek_by(&mut self, secs: i64) {
        let Some(now) = &self.now else { return };
        let pos = self.engine.position().as_secs_f64() + secs as f64;
        let max = now.duration.as_secs_f64().max(0.0);
        let target = pos.clamp(0.0, (max - 0.25).max(0.0));
        self.engine.send(Command::Seek(Duration::from_secs_f64(target)));
    }

    fn on_engine_event(&mut self, ev: Event) {
        match ev {
            Event::Loaded { path, duration } => {
                self.fail_streak = 0;
                if let Some(n) = &mut self.now {
                    if self.library.get(n.idx).is_some_and(|t| t.path == path) {
                        if let Some(d) = duration.filter(|d| !d.is_zero()) {
                            n.duration = d;
                        }
                    }
                }
            }
            Event::Finished { path } => {
                if self.current_track().is_some_and(|t| t.path == path) {
                    self.advance(Advance::Auto);
                }
            }
            Event::LoadFailed { path, message } => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.set_status(StatusKind::Error, format!("Can't play {name}: {message}"));
                self.fail_streak += 1;
                if self.fail_streak >= self.queue.len().max(1) {
                    self.now = None;
                    self.fail_streak = 0;
                } else {
                    self.advance(Advance::User);
                }
            }
            Event::SeekFailed(e) => {
                self.set_status(StatusKind::Warn, format!("Seek not supported here: {e}"))
            }
            Event::OutputError(e) => self.set_status(StatusKind::Error, format!("Audio output error: {e}")),
        }
    }

    // ───────────────────────────── selection helpers ─────────────────────────────

    /// Resolves stored references to library indices, keeping list positions.
    pub fn resolve_refs(&self, refs: &[TrackRef]) -> Vec<Option<TrackIdx>> {
        refs.iter().map(|r| self.library.find(&r.path)).collect()
    }

    fn refs_playable(&self, refs: &[TrackRef], selected: usize) -> Option<(Vec<TrackIdx>, usize)> {
        let resolved = self.resolve_refs(refs);
        resolved.get(selected).copied().flatten()?;
        let start = resolved[..selected].iter().filter(|r| r.is_some()).count();
        Some((resolved.into_iter().flatten().collect(), start))
    }

    /// Tracks targeted by track-level actions (favorite, enqueue, add to playlist).
    pub fn selected_tracks(&self) -> Vec<TrackIdx> {
        let one = |r: Option<&TrackRef>| r.and_then(|r| self.library.find(&r.path)).into_iter().collect();
        if let Some(Overlay::Search(s)) = &self.overlay {
            return s.results.get(s.cursor.selected).copied().into_iter().collect();
        }
        match self.screen {
            Screen::NowPlaying => {
                if self.queue_focused {
                    let up = self.queue_view();
                    up.get(self.queue_cursor.selected).map(|(_, i)| *i).into_iter().collect()
                } else {
                    self.now.as_ref().map(|n| n.idx).into_iter().collect()
                }
            }
            Screen::Library => {
                let v = &self.lib;
                if v.showing_groups() {
                    v.groups.get(v.cursor.selected).map(|g| g.tracks.clone()).unwrap_or_default()
                } else {
                    v.tracks.get(v.cursor.selected).copied().into_iter().collect()
                }
            }
            Screen::Playlists => match self.playlists.open {
                Some(p) => one(self
                    .state
                    .playlists
                    .get(p)
                    .and_then(|pl| pl.tracks.get(self.playlists.track_cursor.selected))),
                None => Vec::new(),
            },
            Screen::Favorites => one(self.state.favorites.get(self.favorites_cursor.selected)),
            Screen::Recent => one(self.state.recent.get(self.recent_cursor.selected)),
            Screen::Settings => Vec::new(),
        }
    }

    /// The queue as shown: current item first, then upcoming in play order.
    /// Each entry is `(index into queue items, track)`.
    pub fn queue_view(&self) -> Vec<(usize, TrackIdx)> {
        let mut v: Vec<(usize, TrackIdx)> = Vec::new();
        if let (Some(i), Some(&t)) = (self.queue.current_index(), self.queue.current()) {
            v.push((i, t));
        }
        v.extend(self.queue.upcoming(500).into_iter().map(|(i, &t)| (i, t)));
        v
    }

    pub fn setting_rows(&self) -> Vec<SettingRow> {
        let mut rows = vec![
            SettingRow::Theme,
            SettingRow::Visualizer,
            SettingRow::Sensitivity,
            SettingRow::Fps,
            SettingRow::Peaks,
            SettingRow::Volume,
            SettingRow::SeekStep,
        ];
        rows.extend((0..self.config.music_dirs.len()).map(SettingRow::Folder));
        rows.push(SettingRow::AddFolder);
        rows.push(SettingRow::Rescan);
        rows
    }

    // ───────────────────────────── input ─────────────────────────────

    pub fn handle_key(&mut self, ev: KeyEvent) {
        if matches!(self.phase, Phase::Welcome(_)) {
            self.welcome_key(ev);
            return;
        }
        if self.overlay.is_some() {
            self.overlay_key(ev);
            return;
        }
        if let Some(action) = self.keymap.resolve(&ev) {
            self.dispatch(action);
        }
    }

    fn welcome_key(&mut self, ev: KeyEvent) {
        let ctrl_c = ev.code == KeyCode::Char('c')
            && ev.modifiers.contains(ratatui::crossterm::event::KeyModifiers::CONTROL);
        if ctrl_c || (self.scan.is_some() && ev.code == KeyCode::Esc) {
            self.quit = true;
            return;
        }
        if self.scan.is_some() {
            return;
        }
        let Phase::Welcome(w) = &mut self.phase else { return };
        if ev.code == KeyCode::Tab {
            if let Some(c) = complete_dir(&w.input.value) {
                w.input.set(c);
            }
            return;
        }
        let submit = match w.input.handle(&ev) {
            TextOutcome::Cancel => {
                self.quit = true;
                return;
            }
            TextOutcome::Submit => validate_dir(&w.input.value),
            TextOutcome::Edited | TextOutcome::Ignored => {
                w.error = None;
                return;
            }
        };
        match submit {
            Ok(dir) => {
                w.error = None;
                self.config.music_dirs = vec![dir];
                self.save_config();
                self.start_scan(true);
            }
            Err(e) => w.error = Some(e),
        }
    }

    /// Called when a foreground scan completes during the welcome flow.
    pub fn leave_welcome_if_ready(&mut self) {
        if matches!(self.phase, Phase::Welcome(_))
            && self.scan.is_none()
            && !self.config.music_dirs.is_empty()
        {
            self.phase = Phase::Main;
            self.screen = if self.library.is_empty() { Screen::Settings } else { Screen::Library };
        }
    }

    fn overlay_key(&mut self, ev: KeyEvent) {
        let action = self.keymap.resolve(&ev);
        let Some(overlay) = self.overlay.take() else { return };
        self.overlay = match overlay {
            Overlay::Help { scroll } => match action {
                Some(Action::Down) => Some(Overlay::Help { scroll: scroll + 1 }),
                Some(Action::Up) => Some(Overlay::Help { scroll: scroll.saturating_sub(1) }),
                Some(Action::PageDown) => Some(Overlay::Help { scroll: scroll + 10 }),
                Some(Action::PageUp) => Some(Overlay::Help { scroll: scroll.saturating_sub(10) }),
                Some(Action::Quit) if ev.code != KeyCode::Char('q') => {
                    self.quit = true;
                    None
                }
                _ => None, // any other key closes help
            },
            Overlay::Search(s) => self.search_key(s, ev, action),
            Overlay::Prompt(p) => self.prompt_key(p, ev),
            Overlay::PlaylistPicker { mut cursor, tracks } => {
                let len = self.state.playlists.len() + 1;
                match action {
                    Some(Action::Down) => cursor.move_by(1, len),
                    Some(Action::Up) => cursor.move_by(-1, len),
                    Some(Action::Top) => cursor.top(),
                    Some(Action::Bottom) => cursor.bottom(len),
                    Some(Action::Select) => {
                        if cursor.selected == 0 {
                            self.overlay = Some(Overlay::Prompt(Prompt {
                                title: "NEW PLAYLIST",
                                input: TextInput::default(),
                                purpose: PromptPurpose::NewPlaylist { then_add: tracks },
                                error: None,
                            }));
                            return;
                        }
                        self.add_paths_to_playlist(cursor.selected - 1, &tracks);
                        return;
                    }
                    Some(Action::Back) | Some(Action::Quit) => return,
                    _ => {}
                }
                Some(Overlay::PlaylistPicker { cursor, tracks })
            }
            Overlay::Confirm { message, action: what } => match ev.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.confirm(what);
                    None
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Char('q') => None,
                _ => Some(Overlay::Confirm { message, action: what }),
            },
        };
    }

    fn search_key(&mut self, mut s: SearchState, ev: KeyEvent, action: Option<Action>) -> Option<Overlay> {
        let len = s.results.len();
        let nav = |s: &mut SearchState, code: KeyCode| -> bool {
            match code {
                KeyCode::Down => s.cursor.move_by(1, len),
                KeyCode::Up => s.cursor.move_by(-1, len),
                KeyCode::PageDown => s.cursor.move_by(10, len),
                KeyCode::PageUp => s.cursor.move_by(-10, len),
                _ => return false,
            }
            true
        };
        if !s.results_focused {
            if nav(&mut s, ev.code) {
                return Some(Overlay::Search(s));
            }
            if ev.code == KeyCode::Tab {
                s.results_focused = !s.results.is_empty();
                return Some(Overlay::Search(s));
            }
            match s.input.handle(&ev) {
                TextOutcome::Cancel => return None,
                TextOutcome::Submit => {
                    if !s.results.is_empty() {
                        self.play_list(s.results.clone(), s.cursor.selected);
                        return None;
                    }
                }
                TextOutcome::Edited => {
                    s.results = self.library.search(&s.input.value);
                    s.cursor = ListCursor::default();
                }
                TextOutcome::Ignored => {}
            }
            return Some(Overlay::Search(s));
        }
        // Results focused: normal list keys and track actions apply.
        if ev.code == KeyCode::Tab || action == Some(Action::Search) {
            s.results_focused = false;
            return Some(Overlay::Search(s));
        }
        match action {
            Some(Action::Down) => s.cursor.move_by(1, len),
            Some(Action::Up) => s.cursor.move_by(-1, len),
            Some(Action::PageDown) => s.cursor.move_by(10, len),
            Some(Action::PageUp) => s.cursor.move_by(-10, len),
            Some(Action::Top) => s.cursor.top(),
            Some(Action::Bottom) => s.cursor.bottom(len),
            Some(Action::Select) => {
                self.play_list(s.results.clone(), s.cursor.selected);
                return None;
            }
            Some(Action::Back) => return None,
            Some(
                a @ (Action::Enqueue | Action::PlayNext | Action::ToggleFavorite | Action::AddToPlaylist),
            ) => {
                self.overlay = Some(Overlay::Search(s));
                self.dispatch(a);
                return self.overlay.take();
            }
            Some(
                a @ (Action::PlayPause
                | Action::Next
                | Action::Previous
                | Action::SeekBack
                | Action::SeekForward
                | Action::VolumeUp
                | Action::VolumeDown
                | Action::Mute),
            ) => self.dispatch(a),
            Some(Action::Quit) => {
                self.quit = true;
                return None;
            }
            _ => {}
        }
        Some(Overlay::Search(s))
    }

    fn prompt_key(&mut self, mut p: Prompt, ev: KeyEvent) -> Option<Overlay> {
        if ev.code == KeyCode::Tab && p.purpose == PromptPurpose::AddFolder {
            if let Some(c) = complete_dir(&p.input.value) {
                p.input.set(c);
            }
            return Some(Overlay::Prompt(p));
        }
        match p.input.handle(&ev) {
            TextOutcome::Cancel => None,
            TextOutcome::Submit => {
                let value = p.input.value.trim().to_string();
                let result: Result<(), String> = match &p.purpose {
                    PromptPurpose::NewPlaylist { then_add } => match self.state.create_playlist(&value) {
                        Ok(idx) => {
                            let then = then_add.clone();
                            self.mark_dirty();
                            self.playlists.cursor.select(idx, self.state.playlists.len());
                            if then.is_empty() {
                                self.info(format!("Created playlist “{value}”"));
                            } else {
                                self.add_paths_to_playlist(idx, &then);
                            }
                            Ok(())
                        }
                        Err(e) => Err(e.to_string()),
                    },
                    PromptPurpose::RenamePlaylist(idx) => {
                        let idx = *idx;
                        self.state.rename_playlist(idx, &value).map_err(|e| e.to_string()).map(|_| {
                            self.mark_dirty();
                            self.info(format!("Renamed to “{value}”"));
                        })
                    }
                    PromptPurpose::AddFolder => validate_dir(&value).map(|dir| {
                        if !self.config.music_dirs.contains(&dir) {
                            self.config.music_dirs.push(dir);
                            self.save_config();
                            self.start_scan(false);
                            self.info("Folder added — scanning…");
                        }
                    }),
                };
                match result {
                    Ok(()) => None,
                    Err(e) => {
                        p.error = Some(e);
                        Some(Overlay::Prompt(p))
                    }
                }
            }
            TextOutcome::Edited | TextOutcome::Ignored => {
                p.error = None;
                Some(Overlay::Prompt(p))
            }
        }
    }

    fn confirm(&mut self, what: ConfirmAction) {
        match what {
            ConfirmAction::DeletePlaylist(idx) => {
                if let Ok(p) = self.state.delete_playlist(idx) {
                    self.mark_dirty();
                    self.playlists.open = None;
                    self.playlists.cursor.clamp(self.state.playlists.len());
                    self.info(format!("Deleted “{}”", p.name));
                }
            }
            ConfirmAction::RemoveFolder(i) => {
                if i < self.config.music_dirs.len() {
                    let dir = self.config.music_dirs.remove(i);
                    self.save_config();
                    self.info(format!("Removed {} — rescanning…", dir.display()));
                    if self.config.music_dirs.is_empty() {
                        self.finish_scan(Default::default());
                    } else {
                        self.start_scan(false);
                    }
                }
            }
        }
    }

    fn add_paths_to_playlist(&mut self, pl: usize, paths: &[PathBuf]) {
        let mut added = 0;
        for p in paths {
            if let Some(t) = self.library.find(p).and_then(|i| self.library.get(i)).cloned() {
                if self.state.add_to_playlist(pl, &t) == Ok(true) {
                    added += 1;
                }
            }
        }
        self.mark_dirty();
        let name = self.state.playlists.get(pl).map(|p| p.name.clone()).unwrap_or_default();
        match added {
            0 => self.info(format!("Already in “{name}”")),
            1 => self.info(format!("Added to “{name}”")),
            n => self.info(format!("Added {n} tracks to “{name}”")),
        }
    }

    // ───────────────────────────── actions ─────────────────────────────

    pub fn dispatch(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::Help => self.overlay = Some(Overlay::Help { scroll: 0 }),
            Action::Search => {
                self.overlay = Some(Overlay::Search(SearchState {
                    input: TextInput::default(),
                    results: Vec::new(),
                    cursor: ListCursor::default(),
                    results_focused: false,
                }))
            }
            Action::Screen(n) => {
                if let Some(s) = Screen::ALL.get((n as usize).wrapping_sub(1)) {
                    self.screen = *s;
                }
            }
            Action::PlayPause => self.play_pause(),
            Action::Stop => self.stop(),
            Action::Next => self.advance(Advance::User),
            Action::Previous => self.previous(),
            Action::SeekBack | Action::SeekForward if self.screen == Screen::Settings => {
                self.adjust_setting(if action == Action::SeekBack { -1 } else { 1 })
            }
            Action::SeekBack => self.seek_by(-(self.config.seek_seconds as i64)),
            Action::SeekForward => self.seek_by(self.config.seek_seconds as i64),
            Action::VolumeUp | Action::VolumeDown => {
                self.volume = if action == Action::VolumeUp {
                    self.volume.saturating_add(VOLUME_STEP).min(100)
                } else {
                    self.volume.saturating_sub(VOLUME_STEP)
                };
                self.muted = false;
                self.apply_volume();
                self.mark_dirty();
            }
            Action::Mute => {
                self.muted = !self.muted;
                self.apply_volume();
                self.mark_dirty();
            }
            Action::ToggleShuffle => {
                let on = self.queue.toggle_shuffle();
                self.queue_cursor = ListCursor::default();
                self.info(if on { "Shuffle on" } else { "Shuffle off" });
            }
            Action::CycleRepeat => {
                let r = self.queue.cycle_repeat();
                self.info(format!("Repeat {}", r.label().to_lowercase()));
            }
            Action::CycleVisualizer => {
                self.visualizer = self.visualizer.next();
                self.mark_dirty();
                self.info(format!("Visualizer · {}", self.visualizer.label().to_lowercase()));
            }
            Action::CycleTheme => self.set_theme(self.config.theme.next()),
            Action::ToggleFavorite => self.toggle_favorite(),
            Action::Enqueue | Action::PlayNext => {
                let tracks = self.selected_tracks();
                if tracks.is_empty() {
                    return;
                }
                let n = tracks.len();
                if action == Action::Enqueue {
                    for t in tracks {
                        self.queue.push(t);
                    }
                } else {
                    for t in tracks.into_iter().rev() {
                        self.queue.push_next(t);
                    }
                }
                let what = if n == 1 { "1 track".to_string() } else { format!("{n} tracks") };
                self.info(if action == Action::Enqueue {
                    format!("Queued {what}")
                } else {
                    format!("Playing {what} next")
                });
            }
            Action::AddToPlaylist => {
                let tracks: Vec<PathBuf> = self
                    .selected_tracks()
                    .iter()
                    .filter_map(|&i| self.library.get(i))
                    .map(|t| t.path.clone())
                    .collect();
                if !tracks.is_empty() {
                    self.overlay = Some(Overlay::PlaylistPicker { cursor: ListCursor::default(), tracks });
                }
            }
            Action::JumpToCurrent => self.jump_to_current(),
            _ => self.screen_action(action),
        }
    }

    fn set_theme(&mut self, t: ThemeName) {
        self.config.theme = t;
        self.theme = Theme::new(t, self.color_depth);
        self.save_config();
        self.info(format!("Theme · {}", t.label()));
    }

    fn toggle_favorite(&mut self) {
        let tracks = self.selected_tracks();
        let Some(t) = tracks.first().and_then(|&i| self.library.get(i)).cloned() else { return };
        if tracks.len() > 1 {
            // A whole group: favorite all if any isn't, otherwise unfavorite all.
            let all =
                tracks.iter().filter_map(|&i| self.library.get(i)).all(|t| self.state.is_favorite(&t.path));
            for t in tracks.iter().filter_map(|&i| self.library.get(i)).cloned().collect::<Vec<_>>() {
                if self.state.is_favorite(&t.path) == all {
                    self.state.toggle_favorite(&t);
                }
            }
            self.info(if all { "Removed from favorites" } else { "Added to favorites" });
        } else {
            let on = self.state.toggle_favorite(&t);
            self.info(if on { format!("♥ {}", t.title) } else { format!("Unfavorited {}", t.title) });
        }
        self.favorites_cursor.clamp(self.state.favorites.len());
        self.mark_dirty();
    }

    fn jump_to_current(&mut self) {
        let Some(n) = &self.now else { return };
        let idx = n.idx;
        match self.screen {
            Screen::Library => {
                if self.lib.showing_groups() {
                    if let Some(g) = self.lib.groups.iter().position(|g| g.tracks.contains(&idx)) {
                        self.lib.cursor.select(g, self.lib.groups.len());
                    }
                } else if let Some(p) = self.lib.tracks.iter().position(|&t| t == idx) {
                    self.lib.cursor.select(p, self.lib.tracks.len());
                }
            }
            Screen::NowPlaying => {
                self.queue_focused = true;
                self.queue_cursor = ListCursor::default();
            }
            _ => {}
        }
    }

    /// List navigation and context actions for the current screen.
    fn screen_action(&mut self, action: Action) {
        match self.screen {
            Screen::NowPlaying => self.now_playing_action(action),
            Screen::Library => self.library_action(action),
            Screen::Playlists => self.playlists_action(action),
            Screen::Favorites => {
                let len = self.state.favorites.len();
                match action {
                    Action::Select => {
                        if let Some((items, start)) =
                            self.refs_playable(&self.state.favorites, self.favorites_cursor.selected)
                        {
                            self.play_list(items, start);
                        } else {
                            self.set_status(
                                StatusKind::Warn,
                                "That file is missing — it may have been moved or deleted",
                            );
                        }
                    }
                    Action::Delete => {
                        if self.favorites_cursor.selected < len {
                            self.state.favorites.remove(self.favorites_cursor.selected);
                            self.favorites_cursor.clamp(len - 1);
                            self.mark_dirty();
                        }
                    }
                    a => nav(&mut self.favorites_cursor, a, len),
                }
            }
            Screen::Recent => {
                let len = self.state.recent.len();
                match action {
                    Action::Select => {
                        if let Some((items, start)) =
                            self.refs_playable(&self.state.recent, self.recent_cursor.selected)
                        {
                            self.play_list(items, start);
                        } else {
                            self.set_status(
                                StatusKind::Warn,
                                "That file is missing — it may have been moved or deleted",
                            );
                        }
                    }
                    a => nav(&mut self.recent_cursor, a, len),
                }
            }
            Screen::Settings => self.settings_action(action),
        }
    }

    fn now_playing_action(&mut self, action: Action) {
        let view = self.queue_view();
        let len = view.len();
        match action {
            Action::NextPanel | Action::PrevPanel => self.queue_focused = !self.queue_focused,
            Action::Back => self.queue_focused = false,
            Action::Select if self.queue_focused => {
                if let Some(&(qi, _)) = view.get(self.queue_cursor.selected) {
                    self.fail_streak = 0;
                    self.queue.jump(qi);
                    self.queue_cursor = ListCursor::default();
                    self.load_current();
                }
            }
            Action::Delete if self.queue_focused => {
                if let Some(&(qi, _)) = view.get(self.queue_cursor.selected) {
                    if self.queue.remove(qi) {
                        if self.queue.current().is_some() {
                            self.load_current();
                        } else {
                            self.stop();
                        }
                    }
                    self.queue_cursor.clamp(len.saturating_sub(1));
                }
            }
            a @ (Action::Up
            | Action::Down
            | Action::Top
            | Action::Bottom
            | Action::PageUp
            | Action::PageDown) => {
                self.queue_focused = true;
                nav(&mut self.queue_cursor, a, len);
            }
            _ => {}
        }
    }

    fn library_action(&mut self, action: Action) {
        let len = self.lib.len();
        match action {
            Action::NextPanel | Action::PrevPanel => {
                let n = LibTab::ALL.len();
                self.lib.tab_index = if action == Action::NextPanel {
                    (self.lib.tab_index + 1) % n
                } else {
                    (self.lib.tab_index + n - 1) % n
                };
                self.lib.open = None;
                self.lib.cursor = ListCursor::default();
                self.lib.group_cursor = ListCursor::default();
                self.refresh_library_view();
            }
            Action::Select => {
                if self.lib.showing_groups() {
                    if self.lib.cursor.selected < self.lib.groups.len() {
                        self.lib.group_cursor = self.lib.cursor;
                        self.lib.open = Some(self.lib.cursor.selected);
                        self.lib.cursor = ListCursor::default();
                        self.refresh_library_view();
                    }
                } else if !self.lib.tracks.is_empty() {
                    self.play_list(self.lib.tracks.clone(), self.lib.cursor.selected);
                }
            }
            Action::Back => {
                if self.lib.open.take().is_some() {
                    self.lib.cursor = self.lib.group_cursor;
                    self.refresh_library_view();
                }
            }
            Action::Sort => {
                if self.lib.tab() == LibTab::Tracks {
                    self.lib.sort = self.lib.sort.next();
                    self.refresh_library_view();
                    self.info(format!("Sorted by {}", self.lib.sort.label()));
                }
            }
            a => nav(&mut self.lib.cursor, a, len),
        }
    }

    fn playlists_action(&mut self, action: Action) {
        let count = self.state.playlists.len();
        match self.playlists.open {
            None => match action {
                Action::Select | Action::NextPanel => {
                    if self.playlists.cursor.selected < count {
                        self.playlists.open = Some(self.playlists.cursor.selected);
                        self.playlists.track_cursor = ListCursor::default();
                    }
                }
                Action::Create => {
                    self.overlay = Some(Overlay::Prompt(Prompt {
                        title: "NEW PLAYLIST",
                        input: TextInput::default(),
                        purpose: PromptPurpose::NewPlaylist { then_add: Vec::new() },
                        error: None,
                    }))
                }
                Action::Rename => self.rename_selected_playlist(),
                Action::Delete => {
                    if let Some(p) = self.state.playlists.get(self.playlists.cursor.selected) {
                        self.overlay = Some(Overlay::Confirm {
                            message: format!("Delete playlist “{}”?", p.name),
                            action: ConfirmAction::DeletePlaylist(self.playlists.cursor.selected),
                        });
                    }
                }
                a => nav(&mut self.playlists.cursor, a, count),
            },
            Some(p) => {
                let Some(pl) = self.state.playlists.get(p) else {
                    self.playlists.open = None;
                    return;
                };
                let len = pl.tracks.len();
                let sel = self.playlists.track_cursor.selected;
                match action {
                    Action::Back | Action::PrevPanel => self.playlists.open = None,
                    Action::Select => {
                        if let Some((items, start)) = self.refs_playable(&pl.tracks, sel) {
                            self.play_list(items, start);
                        } else if len > 0 {
                            self.set_status(
                                StatusKind::Warn,
                                "That file is missing — it may have been moved or deleted",
                            );
                        }
                    }
                    Action::Delete => {
                        if self.state.remove_from_playlist(p, sel).is_ok() {
                            self.playlists.track_cursor.clamp(len - 1);
                            self.mark_dirty();
                        }
                    }
                    Action::MoveUp | Action::MoveDown => {
                        let d = if action == Action::MoveUp { -1 } else { 1 };
                        if let Some(to) = self.state.move_in_playlist(p, sel, d) {
                            self.playlists.track_cursor.select(to, len);
                            self.mark_dirty();
                        }
                    }
                    Action::Rename => self.rename_selected_playlist(),
                    a => nav(&mut self.playlists.track_cursor, a, len),
                }
            }
        }
    }

    fn rename_selected_playlist(&mut self) {
        let idx = self.playlists.open.unwrap_or(self.playlists.cursor.selected);
        if let Some(p) = self.state.playlists.get(idx) {
            self.overlay = Some(Overlay::Prompt(Prompt {
                title: "RENAME PLAYLIST",
                input: TextInput::new(p.name.clone()),
                purpose: PromptPurpose::RenamePlaylist(idx),
                error: None,
            }));
        }
    }

    fn settings_action(&mut self, action: Action) {
        let rows = self.setting_rows();
        let row = rows.get(self.settings_cursor.selected).copied();
        match action {
            Action::Select => match row {
                Some(SettingRow::AddFolder) => self.open_add_folder(),
                Some(SettingRow::Rescan) => {
                    self.start_scan(false);
                    if self.scan.is_some() {
                        self.info("Rescanning library…");
                    }
                }
                Some(SettingRow::Folder(_)) => {}
                Some(_) => self.adjust_setting(1),
                None => {}
            },
            Action::Create => self.open_add_folder(),
            Action::Delete => {
                if let Some(SettingRow::Folder(i)) = row {
                    self.overlay = Some(Overlay::Confirm {
                        message: format!("Remove {} from the library?", self.config.music_dirs[i].display()),
                        action: ConfirmAction::RemoveFolder(i),
                    });
                }
            }
            a => nav(&mut self.settings_cursor, a, rows.len()),
        }
    }

    fn open_add_folder(&mut self) {
        self.overlay = Some(Overlay::Prompt(Prompt {
            title: "ADD MUSIC FOLDER",
            input: TextInput::default(),
            purpose: PromptPurpose::AddFolder,
            error: None,
        }));
    }

    fn adjust_setting(&mut self, dir: i32) {
        let rows = self.setting_rows();
        let Some(row) = rows.get(self.settings_cursor.selected).copied() else { return };
        let v = &mut self.config.visualizer;
        match row {
            SettingRow::Theme => {
                let all = ThemeName::ALL;
                let i = all.iter().position(|t| *t == self.config.theme).unwrap_or(0) as i32;
                let next = all[(i + dir).rem_euclid(all.len() as i32) as usize];
                self.set_theme(next);
                return;
            }
            SettingRow::Visualizer => {
                let all = VisualizerMode::ALL;
                let i = all.iter().position(|m| *m == self.visualizer).unwrap_or(0) as i32;
                self.visualizer = all[(i + dir).rem_euclid(all.len() as i32) as usize];
                v.mode = self.visualizer;
                self.mark_dirty();
            }
            SettingRow::Sensitivity => {
                v.sensitivity = ((v.sensitivity + 0.25 * dir as f32) * 4.0).round() / 4.0;
                v.sensitivity = v.sensitivity.clamp(SENSITIVITY_MIN, SENSITIVITY_MAX);
            }
            SettingRow::Fps => v.fps = (v.fps as i32 + 5 * dir).clamp(FPS_MIN as i32, FPS_MAX as i32) as u32,
            SettingRow::Peaks => v.peaks = !v.peaks,
            SettingRow::Volume => {
                self.config.volume = (self.config.volume as i32 + 5 * dir).clamp(0, 100) as u8;
                self.volume = self.config.volume;
                self.muted = false;
                self.apply_volume();
                self.mark_dirty();
            }
            SettingRow::SeekStep => {
                self.config.seek_seconds = (self.config.seek_seconds as i64 + dir as i64).clamp(1, 120) as u64
            }
            SettingRow::Folder(_) | SettingRow::AddFolder | SettingRow::Rescan => return,
        }
        self.save_config();
    }

    pub fn on_resize(&mut self) {
        // Cursors re-clamp on next render via scroll_into_view; nothing else is size-dependent.
    }

    pub fn shutdown(&mut self) {
        self.engine.send(Command::Stop);
        self.save_state_now();
    }
}

/// Shared list navigation.
fn nav(c: &mut ListCursor, action: Action, len: usize) {
    match action {
        Action::Down => c.move_by(1, len),
        Action::Up => c.move_by(-1, len),
        Action::PageDown => c.move_by(10, len),
        Action::PageUp => c.move_by(-10, len),
        Action::Top => c.top(),
        Action::Bottom => c.bottom(len),
        _ => {}
    }
}

/// Validates a user-entered music folder.
pub fn validate_dir(raw: &str) -> Result<PathBuf, String> {
    if raw.trim().is_empty() {
        return Err("Enter a folder path".into());
    }
    let p = expand_tilde(raw.trim());
    match std::fs::metadata(&p) {
        Ok(m) if m.is_dir() => Ok(std::fs::canonicalize(&p).unwrap_or(p)),
        Ok(_) => Err("That's a file, not a folder".into()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err("Permission denied".into()),
        Err(_) => Err("Folder not found".into()),
    }
}

/// Completes the last path component to the longest common directory prefix.
pub fn complete_dir(input: &str) -> Option<String> {
    let expanded = expand_tilde(input);
    let s = expanded.to_string_lossy().into_owned();
    let (dir, prefix) = if s.ends_with(std::path::MAIN_SEPARATOR) || s.ends_with('/') {
        (PathBuf::from(&s), String::new())
    } else {
        let p = Path::new(&s);
        (
            p.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
            p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
        )
    };
    let dir = if dir.as_os_str().is_empty() { PathBuf::from(".") } else { dir };
    let mut matches: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&prefix) && (prefix.starts_with('.') || !n.starts_with('.')))
        .collect();
    matches.sort();
    let first = matches.first()?;
    let common: String = if matches.len() == 1 {
        format!("{first}{}", std::path::MAIN_SEPARATOR)
    } else {
        let mut c = first.clone();
        for m in &matches[1..] {
            while !m.starts_with(&c) {
                c.pop();
            }
        }
        c
    };
    Some(dir.join(common).to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_directories() {
        let d = tempfile::tempdir().unwrap();
        assert!(validate_dir(d.path().to_str().unwrap()).is_ok());
        assert_eq!(validate_dir("   "), Err("Enter a folder path".into()));
        assert_eq!(validate_dir("/definitely/not/here/xyz"), Err("Folder not found".into()));
        let f = d.path().join("file.txt");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(validate_dir(f.to_str().unwrap()), Err("That's a file, not a folder".into()));
    }

    #[test]
    fn completes_directory_names() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("Music")).unwrap();
        std::fs::create_dir(d.path().join("Musicals")).unwrap();
        std::fs::create_dir(d.path().join("Podcasts")).unwrap();
        let base = d.path().to_string_lossy().into_owned();
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(complete_dir(&format!("{base}{sep}Po")), Some(format!("{base}{sep}Podcasts{sep}")));
        assert_eq!(complete_dir(&format!("{base}{sep}Mu")), Some(format!("{base}{sep}Music")));
        assert_eq!(complete_dir(&format!("{base}{sep}Zz")), None);
    }
}
