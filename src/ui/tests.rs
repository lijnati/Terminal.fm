//! Rendering and interaction tests using ratatui's `TestBackend`.

use std::path::PathBuf;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

use crate::app::{App, AppInit, Overlay, Phase, Screen};
use crate::audio::EngineOptions;
use crate::library::Library;
use crate::library::track::{RawTags, Track};
use crate::storage::{Config, State, ThemeName, VisualizerMode};
use crate::ui::theme::ColorDepth;

fn track(n: u32, title: &str, artist: &str, album: &str) -> Track {
    Track::from_tags(
        PathBuf::from(format!("/music/{artist}/{album}/{n:02} {title}.mp3")),
        RawTags {
            title: Some(title.into()),
            artist: Some(artist.into()),
            album: Some(album.into()),
            track_no: Some(n),
            genre: Some("Electronic".into()),
            year: Some(2011),
            duration: Duration::from_secs(180 + n as u64),
            ..Default::default()
        },
        0,
    )
}

fn app_with(tracks: Vec<Track>, first_run: bool) -> App {
    let config = Config { music_dirs: vec![PathBuf::from("/music")], ..Default::default() };
    App::new(AppInit {
        config,
        config_writable: false,
        first_run,
        paths: None,
        state: State::default(),
        library: Library::new(tracks),
        engine: EngineOptions { null_output: true },
        color_depth: ColorDepth::TrueColor,
        warnings: Vec::new(),
    })
}

fn app() -> App {
    let mut tracks = vec![
        track(1, "Midnight City", "M83", "Hurry Up, We're Dreaming"),
        track(2, "Wait", "M83", "Hurry Up, We're Dreaming"),
        track(1, "Intro", "The xx", "xx"),
        track(3, "Something About Us", "Daft Punk", "Discovery"),
        track(1, "東京フラッシュ", "Vaundy", "strobo"),
    ];
    for i in 0..40 {
        tracks.push(track(
            i + 4,
            &format!("Filler Track With A Rather Long Title {i}"),
            "Various",
            "Long Compilation",
        ));
    }
    app_with(tracks, false)
}

fn key(c: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}
fn code(c: KeyCode) -> KeyEvent {
    KeyEvent { code: c, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE }
}
fn press(app: &mut App, keys: &str) {
    for c in keys.chars() {
        app.handle_key(key(c));
    }
}

fn draw(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    app.update_visualizer();
    term.draw(|f| super::render(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            // A wide glyph covers the following cell(s); skip them like a terminal would.
            let sym = buf[(x, y)].symbol();
            out.push_str(sym);
            x += super::text::width(sym).max(1) as u16;
        }
        out.push('\n');
    }
    out
}

const SIZES: &[(u16, u16)] =
    &[(1, 1), (10, 3), (29, 7), (30, 8), (40, 12), (59, 17), (60, 18), (80, 24), (120, 36), (200, 60)];

#[test]
fn every_screen_and_overlay_renders_at_every_size() {
    let mut a = app();
    a.play_list((0..a.library.len()).collect(), 0);
    a.state.create_playlist("Mix").unwrap();
    let t = a.library.get(0).unwrap().clone();
    a.state.add_to_playlist(0, &t).unwrap();
    a.state.toggle_favorite(&t);
    a.state.push_recent(&t);
    for screen in Screen::ALL {
        a.screen = screen;
        for mode in VisualizerMode::ALL {
            a.visualizer = mode;
            for &(w, h) in SIZES {
                draw(&mut a, w, h);
            }
        }
    }
    a.screen = Screen::Library;
    for keys in ["?", "/mid", "A"] {
        a.overlay = None;
        press(&mut a, keys);
        for &(w, h) in SIZES {
            draw(&mut a, w, h);
        }
    }
    a.overlay = None;
    a.screen = Screen::Playlists;
    press(&mut a, "c");
    assert!(matches!(a.overlay, Some(Overlay::Prompt(_))));
    for &(w, h) in SIZES {
        draw(&mut a, w, h);
    }
    a.overlay = None;
    press(&mut a, "d");
    assert!(matches!(a.overlay, Some(Overlay::Confirm { .. })));
    for &(w, h) in SIZES {
        draw(&mut a, w, h);
    }
}

#[test]
fn welcome_renders_at_every_size() {
    let mut a = app_with(Vec::new(), true);
    assert!(matches!(a.phase, Phase::Welcome(_)));
    for &(w, h) in SIZES {
        let s = draw(&mut a, w, h);
        if w >= 60 && h >= 20 {
            assert!(s.contains("Where is your music?"), "{s}");
        }
    }
}

#[test]
fn tiny_terminal_shows_notice() {
    let mut a = app();
    let s = draw(&mut a, 29, 7);
    assert!(s.contains("Terminal too small"), "{s}");
    assert!(s.contains("need 30×8"));
}

#[test]
fn library_lists_tracks_with_columns() {
    let mut a = app();
    a.screen = Screen::Library;
    let s = draw(&mut a, 120, 36);
    assert!(s.contains("TRACKS") && s.contains("ARTISTS") && s.contains("ALBUMS") && s.contains("GENRES"));
    assert!(s.contains("Midnight City") && s.contains("M83") && s.contains("Hurry Up"));
    assert!(s.contains("03:01"), "durations are shown:\n{s}");
    // Narrow terminals drop columns rather than overflow.
    let narrow = draw(&mut a, 40, 20);
    for line in narrow.lines() {
        assert!(super::text::width(line) <= 40);
    }
    assert!(!narrow.contains("ALBUM"));
}

#[test]
fn wide_characters_do_not_break_alignment() {
    let mut tracks = vec![track(1, "Midnight City", "M83", "Hurry Up")];
    tracks.push(track(1, "東京フラッシュ", "Vaundy", "strobo"));
    let mut a = app_with(tracks, false);
    a.screen = Screen::Library;
    let s = draw(&mut a, 100, 30);
    let row = s.lines().find(|l| l.contains("東京")).expect("CJK title rendered");
    let time_col = s.lines().find(|l| l.contains("Midnight City")).unwrap().find("03:01").unwrap();
    // Same display column for the duration on both rows.
    let cjk_time = row.find("03:01").unwrap();
    assert_eq!(
        super::text::width(&row[..cjk_time]),
        super::text::width(&s.lines().find(|l| l.contains("Midnight City")).unwrap()[..time_col])
    );
}

#[test]
fn now_playing_shows_track_queue_and_honest_state() {
    let mut a = app();
    let s = draw(&mut a, 120, 36);
    assert!(s.contains("Nothing playing"), "{s}");
    a.play_list(vec![0, 1, 2], 0);
    let s = draw(&mut a, 120, 36);
    let title = a.current_track().unwrap().title.clone();
    assert!(s.contains(&title));
    assert!(s.contains("QUEUE") && s.contains("1 of 3"));
    assert!(s.contains("SHUFFLE OFF") && s.contains("REPEAT OFF") && s.contains("VOL 75%"));
    // The fake tracks have no audio files: the engine reports a load failure
    // and the visualizer must not pretend there's signal.
    assert!(!s.contains("SILENCE"));
}

#[test]
fn search_typing_never_triggers_shortcuts() {
    let mut a = app();
    press(&mut a, "/");
    // q, space, n, v would quit / play / next / switch visualizer outside the field.
    press(&mut a, "q v n");
    assert!(!a.quit);
    assert_eq!(a.visualizer, VisualizerMode::Spectrum);
    match &a.overlay {
        Some(Overlay::Search(s)) => assert_eq!(s.input.value, "q v n"),
        other => panic!("search should be open: {other:?}"),
    }
    a.handle_key(code(KeyCode::Esc));
    assert!(a.overlay.is_none());
    press(&mut a, "/daft");
    let Some(Overlay::Search(s)) = &a.overlay else { panic!() };
    assert_eq!(s.results.len(), 1);
    let s = draw(&mut a, 100, 30);
    assert!(s.contains("Something About Us") && s.contains("1 result"));
    a.handle_key(code(KeyCode::Enter));
    assert!(a.overlay.is_none());
    assert_eq!(a.current_track().unwrap().title, "Something About Us");
}

#[test]
fn keyboard_navigation_and_playback_controls() {
    let mut a = app();
    press(&mut a, "2");
    assert_eq!(a.screen, Screen::Library);
    press(&mut a, "jjj");
    assert_eq!(a.lib.cursor.selected, 3);
    press(&mut a, "k");
    assert_eq!(a.lib.cursor.selected, 2);
    press(&mut a, "G");
    assert_eq!(a.lib.cursor.selected, a.library.len() - 1);
    press(&mut a, "g");
    assert_eq!(a.lib.cursor.selected, 0);
    a.handle_key(code(KeyCode::Enter));
    assert!(a.now.is_some());
    press(&mut a, "s");
    assert!(a.queue.shuffle());
    press(&mut a, "rr");
    assert_eq!(a.queue.repeat(), crate::playback::Repeat::One);
    press(&mut a, "--");
    assert_eq!(a.volume, 65);
    press(&mut a, "+");
    assert_eq!(a.volume, 70);
    press(&mut a, "m");
    assert!(a.muted);
    press(&mut a, "v");
    assert_eq!(a.visualizer, VisualizerMode::Waveform);
    press(&mut a, "t");
    assert_eq!(a.config.theme, ThemeName::Amber);
    press(&mut a, "f");
    assert_eq!(a.state.favorites.len(), 1);
    press(&mut a, "?");
    assert!(matches!(a.overlay, Some(Overlay::Help { .. })));
    press(&mut a, "x"); // any key closes help
    assert!(a.overlay.is_none());
    press(&mut a, "q");
    assert!(a.quit);
}

#[test]
fn library_tabs_drill_into_groups() {
    let mut a = app();
    press(&mut a, "2");
    a.handle_key(code(KeyCode::Tab)); // artists
    let s = draw(&mut a, 100, 30);
    assert!(s.contains("Daft Punk") && s.contains("Various"));
    let m83 = a.lib.groups.iter().position(|g| g.name == "M83").unwrap();
    a.lib.cursor.selected = m83;
    a.handle_key(code(KeyCode::Enter));
    assert_eq!(a.lib.tracks.len(), 2);
    let s = draw(&mut a, 100, 30);
    assert!(s.contains("‹ M83") && s.contains("Wait"));
    a.handle_key(code(KeyCode::Esc));
    assert!(a.lib.open.is_none());
    assert_eq!(a.lib.cursor.selected, m83, "cursor restored after leaving a group");
}

#[test]
fn playlist_workflow_via_keys() {
    let mut a = app();
    press(&mut a, "2A");
    // Row 0 of the picker is "new playlist".
    a.handle_key(code(KeyCode::Enter));
    press(&mut a, "Road Trip");
    a.handle_key(code(KeyCode::Enter));
    assert_eq!(a.state.playlists.len(), 1);
    assert_eq!(a.state.playlists[0].tracks.len(), 1);
    press(&mut a, "jA");
    a.handle_key(code(KeyCode::Down));
    a.handle_key(code(KeyCode::Enter));
    assert_eq!(a.state.playlists[0].tracks.len(), 2);
    press(&mut a, "3");
    a.handle_key(code(KeyCode::Enter));
    assert_eq!(a.playlists.open, Some(0));
    press(&mut a, "jK");
    assert_eq!(a.playlists.track_cursor.selected, 0);
    let s = draw(&mut a, 100, 30);
    assert!(s.contains("ROAD TRIP"));
    press(&mut a, "d");
    assert_eq!(a.state.playlists[0].tracks.len(), 1);
    a.handle_key(code(KeyCode::Esc));
    press(&mut a, "d");
    assert!(matches!(a.overlay, Some(Overlay::Confirm { .. })));
    press(&mut a, "y");
    assert!(a.state.playlists.is_empty());
    let s = draw(&mut a, 100, 30);
    assert!(s.contains("No playlists yet"));
}

#[test]
fn missing_files_render_as_missing_in_favorites() {
    let mut a = app();
    let ghost = track(9, "Gone Song", "Nobody", "Nowhere");
    a.state.toggle_favorite(&ghost);
    press(&mut a, "4");
    let s = draw(&mut a, 100, 30);
    assert!(s.contains("Gone Song") && s.contains("missing"));
    a.handle_key(code(KeyCode::Enter));
    assert!(a.now.is_none(), "missing files aren't played");
    assert!(a.status.as_ref().is_some_and(|s| s.text.contains("missing")));
}

#[test]
fn empty_states_explain_what_to_do() {
    let mut a = app_with(Vec::new(), false);
    for (screen, text) in [
        (Screen::Library, "Your library is empty"),
        (Screen::Playlists, "No playlists yet"),
        (Screen::Favorites, "No favorites yet"),
        (Screen::Recent, "Nothing played yet"),
    ] {
        a.screen = screen;
        let s = draw(&mut a, 100, 30);
        assert!(s.contains(text), "{screen:?}: {s}");
    }
}

#[test]
fn settings_adjust_with_h_l() {
    let mut a = app();
    press(&mut a, "6");
    let s = draw(&mut a, 100, 30);
    assert!(s.contains("APPEARANCE") && s.contains("Midnight"));
    press(&mut a, "l");
    assert_eq!(a.config.theme, ThemeName::Amber);
    press(&mut a, "h");
    assert_eq!(a.config.theme, ThemeName::Midnight);
    press(&mut a, "jjj"); // refresh rate
    press(&mut a, "l");
    assert_eq!(a.config.visualizer.fps, 35);
    // h/l adjust settings here instead of seeking.
    assert!(a.config_writable || a.status.is_some());
}

#[test]
fn themes_render_in_all_color_depths() {
    for depth in [ColorDepth::TrueColor, ColorDepth::Ansi256, ColorDepth::Basic] {
        for name in ThemeName::ALL {
            let mut a = app();
            a.color_depth = depth;
            a.theme = crate::ui::theme::Theme::new(name, depth);
            draw(&mut a, 80, 24);
        }
    }
}
