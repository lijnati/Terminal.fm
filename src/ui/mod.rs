//! Terminal rendering. Reads [`App`] state; the only mutation is caching layout
//! hints (visualizer band count, list scroll offsets).

mod overlays;
mod screens;
pub mod text;
pub mod theme;
pub mod viz;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Widget};

use crate::app::{App, Overlay, Phase, Screen, StatusKind};
use crate::audio::{OutputKind, PlayState};
use crate::input::Action;
use crate::library::format_duration;
use text::{fit, thousands, truncate, width};
use theme::Theme;

/// Below this the app shows a "terminal too small" notice instead of the UI.
pub const MIN_WIDTH: u16 = 30;
pub const MIN_HEIGHT: u16 = 8;

/// Layout density chosen from the terminal size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Density {
    Compact,
    Standard,
    Full,
}

impl Density {
    pub fn for_area(area: Rect) -> Self {
        if area.width >= 110 && area.height >= 28 {
            Density::Full
        } else if area.width >= 60 && area.height >= 18 {
            Density::Standard
        } else {
            Density::Compact
        }
    }
}

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let theme = app.theme.clone();
    f.render_widget(Block::new().style(theme.base()), area);

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        too_small(f.buffer_mut(), area, &theme);
        return;
    }

    if let Phase::Welcome(_) = app.phase {
        overlays::welcome(f, area, app);
        return;
    }

    let density = Density::for_area(area);
    let inner = inset(area, if area.width >= 50 { 2 } else { 1 });
    let mini = app.screen != Screen::NowPlaying;
    let mini_h = if !mini {
        0
    } else if density == Density::Compact {
        1
    } else {
        2
    };
    let rows = split_v(inner, &[1, 1, 0, mini_h, 1]);
    let (header, rule, body, mini_area, footer) = (rows[0], rows[1], rows[2], rows[3], rows[4]);

    draw_header(f.buffer_mut(), header, app, &theme);
    hrule(f.buffer_mut(), rule, &theme);
    let body = Rect { height: body.height.saturating_sub(1), ..body }; // breathing room above footer
    match app.screen {
        Screen::NowPlaying => screens::now_playing(f, body, app, density),
        Screen::Library => screens::library(f, body, app),
        Screen::Playlists => screens::playlists(f, body, app),
        Screen::Favorites => screens::favorites(f, body, app),
        Screen::Recent => screens::recent(f, body, app),
        Screen::Settings => screens::settings(f, body, app),
    }
    if mini {
        mini_player(f.buffer_mut(), mini_area, app, &theme);
    }
    draw_footer(f.buffer_mut(), footer, app, &theme);

    if app.overlay.is_some() {
        overlays::render(f, inner, app);
    }
}

// ───────────────────────────── layout helpers ─────────────────────────────

pub fn inset(r: Rect, x: u16) -> Rect {
    let x = x.min(r.width / 2);
    Rect { x: r.x + x, width: r.width - 2 * x, ..r }
}

/// Splits vertically. The first height of 0 takes the remaining space; any later
/// 0 is a genuinely empty row.
pub fn split_v(area: Rect, heights: &[u16]) -> Vec<Rect> {
    let fixed: u16 = heights.iter().copied().fold(0u16, u16::saturating_add);
    let flex_at = heights.iter().position(|&h| h == 0);
    let flex = area.height.saturating_sub(fixed);
    let mut y = area.y;
    let bottom = area.y + area.height;
    heights
        .iter()
        .enumerate()
        .map(|(i, &h)| {
            let h = if Some(i) == flex_at { flex } else { h };
            let h = h.min(bottom.saturating_sub(y));
            let r = Rect { x: area.x, y, width: area.width, height: h };
            y += h;
            r
        })
        .collect()
}

pub fn line_at(area: Rect, dy: u16) -> Rect {
    if dy >= area.height {
        return Rect { x: area.x, y: area.y + area.height, width: area.width, height: 0 };
    }
    Rect { x: area.x, y: area.y + dy, width: area.width, height: 1 }
}

pub fn hrule(buf: &mut Buffer, area: Rect, theme: &Theme) {
    if area.height == 0 {
        return;
    }
    for x in area.x..area.x + area.width {
        buf[(x, area.y)].set_char('─').set_style(theme.faint());
    }
}

/// Draws `line` and right-aligned `right` on the same row, truncating `line` first.
pub fn line_lr(buf: &mut Buffer, area: Rect, line: Line, right: Line) {
    if area.height == 0 {
        return;
    }
    let rw = right.width() as u16;
    let lw = area.width.saturating_sub(if rw > 0 { rw + 2 } else { 0 });
    line.render(Rect { width: lw, height: 1, ..area }, buf);
    if rw > 0 && rw <= area.width {
        right.render(Rect { x: area.x + area.width - rw, width: rw, height: 1, ..area }, buf);
    }
}

pub fn center_line(buf: &mut Buffer, area: Rect, line: Line) {
    if area.height == 0 {
        return;
    }
    let w = (line.width() as u16).min(area.width);
    line.render(Rect { x: area.x + (area.width - w) / 2, width: w, height: 1, ..area }, buf);
}

fn too_small(buf: &mut Buffer, area: Rect, theme: &Theme) {
    let mid = area.y + area.height / 2;
    let msg = truncate("Terminal too small", area.width as usize);
    let size = truncate(
        &format!("{}×{} · need {MIN_WIDTH}×{MIN_HEIGHT}", area.width, area.height),
        area.width as usize,
    );
    if area.height >= 2 {
        center_line(buf, line_at(area, area.height / 2 - 1), Line::styled(msg, theme.strong()));
        center_line(buf, Rect { y: mid, height: 1, ..area }, Line::styled(size, theme.muted()));
    } else if area.height == 1 {
        center_line(buf, area, Line::styled(msg, theme.strong()));
    }
}

// ───────────────────────────── chrome ─────────────────────────────

fn brand(theme: &Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled("TERMINAL", theme.strong()),
        Span::styled(".FM", theme.accent().add_modifier(Modifier::BOLD)),
    ]
}

fn draw_header(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let mut left = brand(theme);
    left.push(Span::raw("   "));
    let full_nav: usize = Screen::ALL.iter().map(|s| s.label().len() + 4).sum();

    // Right side: output + library state.
    let mut right: Vec<Span> = Vec::new();
    if let Some(job) = &app.scan {
        let txt = match job.discovered {
            Some(total) if total > 0 => format!("scanning {}/{}", thousands(job.done), thousands(total)),
            _ => "scanning…".into(),
        };
        right.push(Span::styled(txt, theme.muted()));
        right.push(Span::styled("  ", theme.muted()));
    }
    match app.engine.output() {
        OutputKind::Device => right.push(Span::styled("● LOCAL", theme.muted())),
        OutputKind::Null(_) => right.push(Span::styled("○ NO OUTPUT", theme.warn())),
    }

    let right_w: usize = right.iter().map(|s| s.width()).sum();
    let budget = (area.width as usize).saturating_sub(13 + right_w + 2);
    if budget >= full_nav {
        for (i, s) in Screen::ALL.iter().enumerate() {
            if *s == app.screen {
                left.push(Span::styled(format!("{}", i + 1), theme.accent()));
                left.push(Span::styled(format!(" {}", s.label().to_uppercase()), theme.strong()));
            } else {
                left.push(Span::styled(format!("{}", i + 1), theme.faint()));
                left.push(Span::styled(format!(" {}", s.label()), theme.muted()));
            }
            left.push(Span::raw("   "));
        }
    } else if budget >= 12 {
        let i = Screen::ALL.iter().position(|s| *s == app.screen).unwrap_or(0);
        left.push(Span::styled(format!("{} ", i + 1), theme.accent()));
        left.push(Span::styled(app.screen.label().to_uppercase(), theme.strong()));
    }
    line_lr(buf, area, Line::from(left), Line::from(right));
}

/// Key hints for the current context, in priority order.
fn hints(app: &App) -> Vec<(String, &'static str)> {
    let k = |a: Action| app.keymap.label(a);
    let mut h: Vec<(String, &'static str)> = Vec::new();
    match &app.overlay {
        Some(Overlay::Search(s)) if !s.results_focused => {
            return vec![
                ("ENTER".into(), "play"),
                ("↑↓".into(), "select"),
                ("TAB".into(), "results"),
                ("ESC".into(), "close"),
            ];
        }
        Some(Overlay::Search(_)) => {
            return vec![
                (k(Action::Select), "play"),
                (k(Action::Enqueue), "queue"),
                (k(Action::AddToPlaylist), "playlist"),
                (k(Action::ToggleFavorite), "favorite"),
                ("TAB".into(), "edit query"),
                ("ESC".into(), "close"),
            ];
        }
        Some(Overlay::Help { .. }) => return vec![("↑↓".into(), "scroll"), ("any key".into(), "close")],
        Some(Overlay::Prompt(_)) => return vec![("ENTER".into(), "confirm"), ("ESC".into(), "cancel")],
        Some(Overlay::PlaylistPicker { .. }) => {
            return vec![(k(Action::Select), "choose"), (k(Action::Back), "cancel")];
        }
        Some(Overlay::Confirm { .. }) => return vec![("y".into(), "yes"), ("n".into(), "no")],
        None => {}
    }
    h.push((k(Action::PlayPause), "play/pause"));
    match app.screen {
        Screen::NowPlaying => {
            h.push((format!("{}/{}", k(Action::Previous), k(Action::Next)), "prev/next"));
            h.push((format!("{}/{}", k(Action::SeekBack), k(Action::SeekForward)), "seek"));
            h.push((k(Action::CycleVisualizer), "visualizer"));
            if app.queue_focused {
                h.push((k(Action::Delete), "remove"));
            }
        }
        Screen::Library => {
            h.push((k(Action::Select), if app.lib.showing_groups() { "open" } else { "play" }));
            h.push((k(Action::NextPanel), "tab"));
            h.push((k(Action::Enqueue), "queue"));
            h.push((k(Action::AddToPlaylist), "playlist"));
            if app.lib.open.is_some() {
                h.push((k(Action::Back), "back"));
            }
        }
        Screen::Playlists => {
            if app.playlists.open.is_some() {
                h.push((k(Action::Select), "play"));
                h.push((format!("{}/{}", k(Action::MoveUp), k(Action::MoveDown)), "reorder"));
                h.push((k(Action::Delete), "remove"));
                h.push((k(Action::Back), "back"));
            } else {
                h.push((k(Action::Select), "open"));
                h.push((k(Action::Create), "new"));
                h.push((k(Action::Rename), "rename"));
                h.push((k(Action::Delete), "delete"));
            }
        }
        Screen::Favorites => {
            h.push((k(Action::Select), "play"));
            h.push((k(Action::Delete), "remove"));
        }
        Screen::Recent => h.push((k(Action::Select), "play")),
        Screen::Settings => {
            h.push((format!("{}/{}", k(Action::SeekBack), k(Action::SeekForward)), "adjust"));
            h.push((k(Action::Create), "add folder"));
            h.push((k(Action::Delete), "remove folder"));
        }
    }
    h.push((k(Action::Search), "search"));
    h.push((k(Action::Help), "help"));
    h.push((k(Action::Quit), "quit"));
    h
}

fn draw_footer(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    if area.height == 0 {
        return;
    }
    if let Some(s) = &app.status {
        let style = match s.kind {
            StatusKind::Info => theme.text(),
            StatusKind::Warn => theme.warn(),
            StatusKind::Error => theme.error(),
        };
        let mark = match s.kind {
            StatusKind::Info => "· ",
            StatusKind::Warn => "▲ ",
            StatusKind::Error => "✕ ",
        };
        let msg = truncate(&format!("{mark}{}", s.text), area.width as usize);
        Line::styled(msg, style).render(area, buf);
        return;
    }
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0usize;
    for (key, label) in hints(app) {
        let w = width(&key) + 1 + width(label) + 3;
        if used + w > area.width as usize + 3 {
            break;
        }
        spans.push(Span::styled(key, theme.strong()));
        spans.push(Span::styled(format!(" {label}   "), theme.muted()));
        used += w;
    }
    Line::from(spans).render(area, buf);
}

/// Progress bar: `━━━━●────`.
pub fn progress_spans(theme: &Theme, ratio: f32, w: usize) -> Vec<Span<'static>> {
    if w == 0 {
        return Vec::new();
    }
    let ratio = if ratio.is_finite() { ratio.clamp(0.0, 1.0) } else { 0.0 };
    let knob = ((w - 1) as f32 * ratio).round() as usize;
    vec![
        Span::styled("━".repeat(knob), theme.accent()),
        Span::styled("●", theme.strong()),
        Span::styled("─".repeat(w - 1 - knob), theme.faint()),
    ]
}

pub fn play_glyph(state: PlayState) -> &'static str {
    match state {
        PlayState::Playing => "▶",
        PlayState::Paused => "❚❚",
        PlayState::Stopped => "■",
    }
}

/// `(position, duration, ratio)` for the playing track.
pub fn timing(app: &App) -> (std::time::Duration, std::time::Duration, f32) {
    let Some(now) = &app.now else { return Default::default() };
    let pos = app.engine.position().min(now.duration.max(app.engine.position()));
    let ratio = if now.duration.is_zero() { 0.0 } else { pos.as_secs_f32() / now.duration.as_secs_f32() };
    (pos, now.duration, ratio)
}

fn mini_player(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    if area.height == 0 {
        return;
    }
    let row = if area.height >= 2 {
        hrule(buf, line_at(area, 0), theme);
        line_at(area, 1)
    } else {
        line_at(area, 0)
    };
    let w = row.width as usize;
    let Some(track) = app.current_track() else {
        let msg = if app.queue.is_empty() { "Nothing playing" } else { "Stopped" };
        line_lr(buf, row, Line::styled(msg, theme.muted()), vol_line(app, theme));
        return;
    };
    let (pos, dur, ratio) = timing(app);
    let state = app.play_state();
    let times = format!("{}  ", format_duration(pos));
    let total = format!("  {}", format_duration(dur));
    // Allocate: info ~45%, progress the rest (if wide enough).
    let right_extra = 12; // volume
    let bar_w = if w >= 70 { (w * 30 / 100).clamp(10, 40) } else { 0 };
    let info_w =
        w.saturating_sub(bar_w + if bar_w > 0 { times.len() + total.len() } else { 0 } + right_extra + 2);
    let info = format!("{} · {}", track.title, track.artist);
    let mut spans = vec![
        Span::styled(format!("{} ", play_glyph(state)), theme.accent()),
        Span::styled(fit(&info, info_w.saturating_sub(3)), theme.text()),
        Span::raw(" "),
    ];
    if bar_w > 0 {
        spans.push(Span::styled(times, theme.muted()));
        spans.extend(progress_spans(theme, ratio, bar_w));
        spans.push(Span::styled(total, theme.muted()));
    }
    line_lr(buf, row, Line::from(spans), vol_line(app, theme));
}

fn vol_line(app: &App, theme: &Theme) -> Line<'static> {
    if app.muted {
        Line::from(vec![Span::styled("VOL ", theme.label()), Span::styled("MUTED", theme.warn())])
    } else {
        Line::from(vec![
            Span::styled("VOL ", theme.label()),
            Span::styled(format!("{}%", app.volume), theme.text()),
        ])
    }
}

/// Style for a selectable row.
pub fn row_style(theme: &Theme, selected: bool, focused: bool) -> Style {
    if selected { theme.selected(focused) } else { theme.text() }
}

#[cfg(test)]
mod tests;
