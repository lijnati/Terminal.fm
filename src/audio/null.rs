//! Realtime null output and stderr silencing for noisy audio backends.

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rodio::mixer::{Mixer, MixerSource};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const TICK: Duration = Duration::from_millis(10);

/// Pulls samples from a mixer at real-time speed and discards them.
pub struct NullClock {
    mixer: Mixer,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl NullClock {
    pub fn start() -> Self {
        let (mixer, source) = rodio::mixer::mixer(
            NonZero::new(CHANNELS).expect("non-zero"),
            NonZero::new(RATE).expect("non-zero"),
        );
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread =
            std::thread::Builder::new().name("terminalfm-null-out".into()).spawn(move || run(source, s)).ok();
        Self { mixer, stop, thread }
    }

    pub fn mixer(&self) -> &Mixer {
        &self.mixer
    }
}

fn run(mut source: MixerSource, stop: Arc<AtomicBool>) {
    let per_tick = (RATE as u64 * CHANNELS as u64 * TICK.as_millis() as u64 / 1000) as usize;
    let mut next = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        for _ in 0..per_tick {
            source.next();
        }
        next += TICK;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else if now - next > Duration::from_millis(200) {
            next = now; // fell far behind (suspended?): don't try to catch up
        }
    }
}

impl Drop for NullClock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Runs `f` with stderr redirected to the null device. ALSA prints diagnostics
/// directly to stderr while probing devices, which would corrupt the TUI.
#[cfg(unix)]
pub fn quiet_stderr<T>(f: impl FnOnce() -> T) -> T {
    use std::os::fd::AsRawFd;
    let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") else {
        return f();
    };
    // SAFETY: plain fd duplication on valid descriptors; the original is restored below.
    let saved = unsafe { libc::dup(2) };
    if saved < 0 {
        return f();
    }
    unsafe { libc::dup2(devnull.as_raw_fd(), 2) };
    let out = f();
    unsafe {
        libc::dup2(saved, 2);
        libc::close(saved);
    }
    out
}

#[cfg(not(unix))]
pub fn quiet_stderr<T>(f: impl FnOnce() -> T) -> T {
    f()
}
