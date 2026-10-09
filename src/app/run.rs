//! Terminal lifecycle and the main event loop.

use std::io;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event as TermEvent, KeyEventKind};

use super::App;
use crate::ui;

/// Restores the terminal however we leave (including early returns via `?`).
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// How long to wait between frames for the current activity.
fn frame_interval(app: &App) -> Duration {
    if app.animating() || app.viz_settling() {
        Duration::from_secs_f64(1.0 / app.config.visualizer.fps.max(1) as f64)
    } else if app.play_state() != crate::audio::PlayState::Stopped || app.scan.is_some() {
        Duration::from_millis(250)
    } else {
        Duration::from_secs(1)
    }
}

pub fn run(app: &mut App) -> io::Result<()> {
    // `try_init` enables raw mode + alternate screen and installs a panic hook that restores both.
    let mut terminal = ratatui::try_init()?;
    let _guard = TerminalGuard;
    let mut last_draw = Instant::now() - Duration::from_secs(1);
    let mut dirty = true;

    while !app.quit {
        app.tick();
        let interval = frame_interval(app);
        if dirty || last_draw.elapsed() >= interval {
            app.update_visualizer();
            terminal.draw(|f| ui::render(f, app))?;
            last_draw = Instant::now();
            dirty = false;
        }
        // Wake for the next frame, but at least every 100 ms to react to engine events.
        let timeout = interval.saturating_sub(last_draw.elapsed()).min(Duration::from_millis(100));
        if event::poll(timeout)? {
            // Drain everything pending so key repeat doesn't queue up redraws.
            loop {
                match event::read()? {
                    TermEvent::Key(k) if k.kind != KeyEventKind::Release => app.handle_key(k),
                    TermEvent::Resize(..) => app.on_resize(),
                    _ => {}
                }
                dirty = true;
                if app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }
    app.shutdown();
    Ok(())
}
