//! Native audio playback engine.
//!
//! Runs on its own thread and is controlled through a bounded command channel.
//! It knows nothing about the terminal UI or the library.
//!
//! Pipeline per track:
//! `File → rodio::Decoder (symphonia) → Tap → Player (pause/volume/seek) → Mixer → output`
//!
//! The output is the default audio device when one can be opened, otherwise a
//! realtime "null" clock that consumes the mixer at the stream rate so that
//! playback state and the visualizer still behave correctly (but nothing is heard).

mod null;
pub mod tap;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use rodio::{Decoder, Player, Source};

pub use tap::TapReader;
use tap::{TapFactory, tap_pair};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputKind {
    /// A real output device.
    Device,
    /// No device: realtime null clock. Carries the reason.
    Null(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped = 0,
    Playing = 1,
    Paused = 2,
}

#[derive(Debug)]
pub enum Command {
    Load {
        path: PathBuf,
        paused: bool,
    },
    Play,
    Pause,
    Stop,
    Seek(Duration),
    /// Linear gain, 0.0–1.0.
    Volume(f32),
    Shutdown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Loaded { path: PathBuf, duration: Option<Duration> },
    LoadFailed { path: PathBuf, message: String },
    Finished { path: PathBuf },
    SeekFailed(String),
    OutputError(String),
}

#[derive(Debug, Default)]
struct Status {
    position_ms: AtomicU64,
    state: AtomicU8,
}

/// Handle to the engine thread. Dropping it shuts the engine down.
pub struct AudioEngine {
    commands: SyncSender<Command>,
    events: Receiver<Event>,
    status: Arc<Status>,
    output: OutputKind,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    /// Skip device probing and use the null output (tests, `--no-audio`).
    pub null_output: bool,
}

impl AudioEngine {
    /// Starts the engine thread. Returns the engine and the visualizer tap reader.
    pub fn start(opts: EngineOptions) -> (Self, TapReader) {
        let (cmd_tx, cmd_rx) = sync_channel(64);
        let (ev_tx, ev_rx) = sync_channel(64);
        let (out_tx, out_rx) = sync_channel(1);
        let (factory, reader) = tap_pair();
        let status = Arc::new(Status::default());
        let st = status.clone();
        let ev_err = ev_tx.clone();
        let thread = std::thread::Builder::new()
            .name("terminalfm-audio".into())
            .spawn(move || engine_thread(opts, cmd_rx, ev_tx, out_tx, factory, st))
            .map_err(|e| {
                let _ = ev_err.try_send(Event::OutputError(e.to_string()));
            })
            .ok();
        let output = out_rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| OutputKind::Null("audio thread did not start".into()));
        (Self { commands: cmd_tx, events: ev_rx, status, output, thread }, reader)
    }

    pub fn output(&self) -> &OutputKind {
        &self.output
    }

    /// Queues a command. Returns false if the engine is gone or saturated.
    pub fn send(&self, cmd: Command) -> bool {
        match self.commands.try_send(cmd) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("audio command dropped: {e}");
                false
            }
        }
    }

    pub fn poll_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    pub fn position(&self) -> Duration {
        Duration::from_millis(self.status.position_ms.load(Ordering::Relaxed))
    }

    pub fn state(&self) -> PlayState {
        match self.status.state.load(Ordering::Relaxed) {
            1 => PlayState::Playing,
            2 => PlayState::Paused,
            _ => PlayState::Stopped,
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.commands.try_send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

enum Output {
    Device(rodio::MixerDeviceSink),
    Null(null::NullClock),
}

impl Output {
    fn mixer(&self) -> &rodio::mixer::Mixer {
        match self {
            Output::Device(s) => s.mixer(),
            Output::Null(n) => n.mixer(),
        }
    }
}

fn open_output(opts: &EngineOptions, events: &SyncSender<Event>) -> (Output, OutputKind) {
    if opts.null_output {
        return (Output::Null(null::NullClock::start()), OutputKind::Null("disabled with --no-audio".into()));
    }
    let ev = events.clone();
    let opened = null::quiet_stderr(|| {
        rodio::DeviceSinkBuilder::from_default_device().and_then(|b| {
            b.with_error_callback(move |e| {
                tracing::error!("audio stream error: {e}");
                let _ = ev.try_send(Event::OutputError(e.to_string()));
            })
            .open_sink_or_fallback()
        })
    });
    match opened {
        Ok(mut sink) => {
            sink.log_on_drop(false);
            (Output::Device(sink), OutputKind::Device)
        }
        Err(e) => {
            tracing::warn!("no audio device, using null output: {e}");
            (Output::Null(null::NullClock::start()), OutputKind::Null(e.to_string()))
        }
    }
}

struct Current {
    player: Player,
    path: PathBuf,
}

fn engine_thread(
    opts: EngineOptions,
    commands: Receiver<Command>,
    events: SyncSender<Event>,
    out_tx: SyncSender<OutputKind>,
    factory: TapFactory,
    status: Arc<Status>,
) {
    let (output, kind) = open_output(&opts, &events);
    let _ = out_tx.send(kind);
    let mut current: Option<Current> = None;
    let mut volume = 1.0f32;
    let set_state = |s: PlayState| status.state.store(s as u8, Ordering::Relaxed);

    loop {
        match commands.recv_timeout(Duration::from_millis(25)) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Command::Load { path, paused }) => {
                current = None; // dropping the old player stops it
                status.position_ms.store(0, Ordering::Relaxed);
                match open_decoder(&path) {
                    Ok(decoder) => {
                        let duration = decoder.total_duration();
                        let player = Player::connect_new(output.mixer());
                        player.set_volume(volume);
                        if paused {
                            player.pause();
                        }
                        player.append(factory.wrap(decoder));
                        set_state(if paused { PlayState::Paused } else { PlayState::Playing });
                        let _ = events.send(Event::Loaded { path: path.clone(), duration });
                        current = Some(Current { player, path });
                    }
                    Err(message) => {
                        set_state(PlayState::Stopped);
                        let _ = events.send(Event::LoadFailed { path, message });
                    }
                }
            }
            Ok(Command::Play) => {
                if let Some(c) = &current {
                    c.player.play();
                    set_state(PlayState::Playing);
                }
            }
            Ok(Command::Pause) => {
                if let Some(c) = &current {
                    c.player.pause();
                    set_state(PlayState::Paused);
                }
            }
            Ok(Command::Stop) => {
                current = None;
                status.position_ms.store(0, Ordering::Relaxed);
                set_state(PlayState::Stopped);
            }
            Ok(Command::Seek(pos)) => {
                if let Some(c) = &current {
                    if let Err(e) = c.player.try_seek(pos) {
                        let _ = events.try_send(Event::SeekFailed(e.to_string()));
                    }
                }
            }
            Ok(Command::Volume(v)) => {
                volume = v.clamp(0.0, 1.0);
                if let Some(c) = &current {
                    c.player.set_volume(volume);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }

        if let Some(c) = &current {
            status.position_ms.store(c.player.get_pos().as_millis() as u64, Ordering::Relaxed);
            if c.player.empty() {
                let path = c.path.clone();
                current = None;
                set_state(PlayState::Stopped);
                let _ = events.send(Event::Finished { path });
            }
        }
    }
    drop(current);
    drop(output);
}

fn open_decoder(path: &std::path::Path) -> Result<Decoder<std::io::BufReader<std::fs::File>>, String> {
    let file = std::fs::File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "file not found (moved or deleted?)".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => e.to_string(),
    })?;
    Decoder::try_from(file).map_err(|e| match e {
        rodio::decoder::DecoderError::UnrecognizedFormat => "unsupported or unrecognized format".to_string(),
        other => other.to_string(),
    })
}
