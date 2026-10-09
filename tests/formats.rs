//! Verifies real codec support: tags via lofty, decoding via rodio/symphonia,
//! and that the analyzer sees the fixture's 440 Hz tone.

mod common;

use std::time::{Duration, Instant};

use terminalfm::analysis::{Analyzer, MAX_FREQ, MIN_FREQ, log_bands};
use terminalfm::audio::{AudioEngine, Command, EngineOptions, Event};
use terminalfm::library::scan::{read_track, scan};

const FORMATS: &[&str] = &["mp3", "flac", "ogg", "m4a", "wav"];

#[test]
fn reads_tags_for_every_supported_format() {
    for ext in FORMATS {
        let path = common::fixtures_dir().join(format!("tone.{ext}"));
        let t = read_track(&path).unwrap_or_else(|e| panic!("{ext}: {e}"));
        assert_eq!(t.title, format!("Tone {ext}"), "{ext}");
        assert_eq!(t.artist, "Fixture Ensemble", "{ext}");
        assert_eq!(t.album, "Test Tones", "{ext}");
        assert_eq!(t.track_no, Some(1), "{ext}");
        assert_eq!(t.genre.as_deref(), Some("Test"), "{ext}");
        assert_eq!(t.year, Some(2024), "{ext}");
        let expect = if *ext == "wav" { 1.0 } else { 2.0 };
        assert!((t.duration.as_secs_f32() - expect).abs() < 0.15, "{ext}: {:?}", t.duration);
    }
}

#[test]
fn scan_reports_corrupt_files_without_aborting() {
    let r = scan(&[common::fixtures_dir()], &[], |_| {});
    let ok: Vec<_> =
        r.tracks.iter().map(|t| t.path.extension().unwrap().to_str().unwrap().to_string()).collect();
    for ext in FORMATS {
        assert!(ok.contains(&ext.to_string()), "{ext} missing from scan: {ok:?}");
    }
    let failed: Vec<_> = r.failures.iter().map(|(p, _)| p.file_name().unwrap().to_str().unwrap()).collect();
    assert!(failed.contains(&"notaudio.flac"), "{failed:?}");
}

#[test]
fn decodes_every_format_and_the_visualizer_sees_440hz() {
    let bands = log_bands(32, MIN_FREQ, MAX_FREQ);
    let want = bands.iter().position(|b| 440.0 >= b.lo && 440.0 < b.hi).unwrap();
    let (engine, mut tap) = AudioEngine::start(EngineOptions { null_output: true });
    for ext in FORMATS {
        let path = common::fixtures_dir().join(format!("tone.{ext}"));
        engine.send(Command::Load { path, paused: false });
        let deadline = Instant::now() + Duration::from_secs(5);
        let loaded = loop {
            if let Some(ev) = engine.poll_event() {
                break ev;
            }
            assert!(Instant::now() < deadline, "{ext}: no event");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(matches!(loaded, Event::Loaded { .. }), "{ext}: {loaded:?}");

        let mut a = Analyzer::new();
        let mut buf = Vec::new();
        for _ in 0..12 {
            std::thread::sleep(Duration::from_millis(33));
            let (ch, sr) = tap.drain(&mut buf);
            a.push(&buf, ch, sr);
            a.update(1.0 / 30.0, 32, 1.0);
        }
        let f = a.frame();
        let peak = f.bands.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1)).unwrap().0;
        assert_eq!(peak, want, "{ext}: spectrum {:?}", f.bands);
    }
}

#[test]
fn truncated_mp3_plays_what_it_can_or_fails_cleanly() {
    let (engine, _tap) = AudioEngine::start(EngineOptions { null_output: true });
    engine.send(Command::Load { path: common::fixtures_dir().join("truncated.mp3"), paused: false });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut events = Vec::new();
    while Instant::now() < deadline {
        if let Some(ev) = engine.poll_event() {
            let done = matches!(ev, Event::Finished { .. } | Event::LoadFailed { .. });
            events.push(ev);
            if done {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(events.last(), Some(Event::Finished { .. } | Event::LoadFailed { .. })), "{events:?}");
}
