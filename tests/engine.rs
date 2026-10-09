//! Playback engine tests using the null (device-less) output.

mod common;

use std::time::{Duration, Instant};

use terminalfm::analysis::{Analyzer, Signal};
use terminalfm::audio::{AudioEngine, Command, EngineOptions, Event, OutputKind, PlayState};

fn engine() -> (AudioEngine, terminalfm::audio::TapReader) {
    AudioEngine::start(EngineOptions { null_output: true })
}

fn wait_event(e: &AudioEngine, timeout: Duration) -> Option<Event> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Some(ev) = e.poll_event() {
            return Some(ev);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn plays_a_wav_to_completion_and_feeds_the_tap() {
    let dir = tempfile::tempdir().unwrap();
    let wav = common::write_sine_wav(&dir.path().join("tone.wav"), 1_000.0, 0.6, 44_100);
    let (e, mut tap) = engine();
    assert!(matches!(e.output(), OutputKind::Null(_)));

    assert!(e.send(Command::Load { path: wav.clone(), paused: false }));
    match wait_event(&e, Duration::from_secs(5)) {
        Some(Event::Loaded { path, duration }) => {
            assert_eq!(path, wav);
            let d = duration.expect("wav duration known");
            assert!((d.as_secs_f32() - 0.6).abs() < 0.05, "{d:?}");
        }
        other => panic!("expected Loaded, got {other:?}"),
    }
    assert_eq!(e.state(), PlayState::Playing);

    // The visualizer tap receives the real decoded samples while playing.
    std::thread::sleep(Duration::from_millis(250));
    let mut buf = Vec::new();
    let (ch, sr) = tap.drain(&mut buf);
    assert_eq!((ch, sr), (2, 44_100));
    assert!(buf.len() > 4_000, "tap got {} samples", buf.len());
    let peak = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!((peak - 0.5).abs() < 0.02, "tap carries the sine at its true amplitude: {peak}");

    let mut a = Analyzer::new();
    a.push(&buf, ch, sr);
    a.update(1.0 / 30.0, 24, 1.0);
    assert_eq!(a.signal(), Signal::Active);

    assert!(e.position() > Duration::from_millis(100));
    match wait_event(&e, Duration::from_secs(5)) {
        Some(Event::Finished { path }) => assert_eq!(path, wav),
        other => panic!("expected Finished, got {other:?}"),
    }
    assert_eq!(e.state(), PlayState::Stopped);
}

#[test]
fn pause_stops_sample_flow_and_seek_moves_position() {
    let dir = tempfile::tempdir().unwrap();
    let wav = common::write_sine_wav(&dir.path().join("long.wav"), 440.0, 6.0, 44_100);
    let (e, mut tap) = engine();
    e.send(Command::Load { path: wav, paused: false });
    assert!(matches!(wait_event(&e, Duration::from_secs(5)), Some(Event::Loaded { .. })));
    std::thread::sleep(Duration::from_millis(100));

    e.send(Command::Pause);
    std::thread::sleep(Duration::from_millis(100));
    let mut buf = Vec::new();
    tap.drain(&mut buf);
    let paused_at = e.position();
    std::thread::sleep(Duration::from_millis(200));
    tap.drain(&mut buf);
    assert!(buf.is_empty(), "no samples flow while paused");
    assert_eq!(e.state(), PlayState::Paused);
    assert!(e.position().abs_diff(paused_at) < Duration::from_millis(30));

    e.send(Command::Seek(Duration::from_secs(4)));
    e.send(Command::Play);
    std::thread::sleep(Duration::from_millis(150));
    assert!(e.position() >= Duration::from_secs(4), "{:?}", e.position());
    assert_eq!(e.state(), PlayState::Playing);

    e.send(Command::Stop);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(e.state(), PlayState::Stopped);
    assert_eq!(e.position(), Duration::ZERO);
}

#[test]
fn corrupt_and_missing_files_fail_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.mp3");
    std::fs::write(&bad, b"this is not audio at all, just text pretending").unwrap();
    let (e, _tap) = engine();

    e.send(Command::Load { path: bad.clone(), paused: false });
    match wait_event(&e, Duration::from_secs(5)) {
        Some(Event::LoadFailed { path, .. }) => assert_eq!(path, bad),
        other => panic!("expected LoadFailed, got {other:?}"),
    }

    let missing = dir.path().join("gone.flac");
    e.send(Command::Load { path: missing, paused: false });
    match wait_event(&e, Duration::from_secs(5)) {
        Some(Event::LoadFailed { message, .. }) => assert!(message.contains("not found"), "{message}"),
        other => panic!("expected LoadFailed, got {other:?}"),
    }
    assert_eq!(e.state(), PlayState::Stopped);
}

#[test]
fn loading_a_new_track_replaces_the_old_one() {
    let dir = tempfile::tempdir().unwrap();
    let a = common::write_sine_wav(&dir.path().join("a.wav"), 440.0, 3.0, 44_100);
    let b = common::write_sine_wav(&dir.path().join("b.wav"), 880.0, 3.0, 48_000);
    let (e, mut tap) = engine();
    e.send(Command::Load { path: a, paused: false });
    wait_event(&e, Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(100));
    e.send(Command::Load { path: b.clone(), paused: false });
    assert!(matches!(wait_event(&e, Duration::from_secs(5)), Some(Event::Loaded { path, .. }) if path == b));
    std::thread::sleep(Duration::from_millis(200));
    let mut buf = Vec::new();
    tap.drain(&mut buf);
    std::thread::sleep(Duration::from_millis(100));
    let (_, sr) = tap.drain(&mut buf);
    assert_eq!(sr, 48_000, "tap reports the new track's format");
    assert!(e.position() < Duration::from_millis(600));
}
