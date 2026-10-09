//! Modal overlays and the first-run welcome screen.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Widget};

use super::text::{fit, fit_right, plural, thousands, truncate, width};
use super::theme::Theme;
use super::{center_line, hrule, line_at, progress_spans, row_style, split_v};
use crate::app::{App, Overlay, Phase};
use crate::input::{Group, TextInput};
use crate::library::format_duration;

/// A centered panel with a hairline border; returns the inner area.
fn panel(f: &mut Frame, area: Rect, w: u16, h: u16, title: &str, theme: &Theme) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
    f.render_widget(Clear, r);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.faint())
        .style(theme.base())
        .title(Span::styled(format!(" {title} "), theme.label()));
    let inner = block.inner(r);
    f.render_widget(block, r);
    Rect { x: inner.x + 1, width: inner.width.saturating_sub(2), ..inner }
}

/// Renders a text field with a visible cursor.
fn input_line(input: &TextInput, w: usize, theme: &Theme, prompt: &str) -> Line<'static> {
    let chars: Vec<char> = input.value.chars().collect();
    let avail = w.saturating_sub(width(prompt) + 1).max(1);
    // Scroll horizontally so the cursor stays visible.
    let start = input.cursor.saturating_sub(avail - 1);
    let before: String = chars[start..input.cursor].iter().collect();
    let at: String = chars.get(input.cursor).map(|c| c.to_string()).unwrap_or_else(|| " ".into());
    let after: String = chars.iter().skip(input.cursor + 1).collect();
    let after = truncate(&after, avail.saturating_sub(width(&before) + 1));
    Line::from(vec![
        Span::styled(prompt.to_string(), theme.accent()),
        Span::styled(before, theme.strong()),
        Span::styled(at, theme.strong().add_modifier(Modifier::REVERSED)),
        Span::styled(after, theme.strong()),
    ])
}

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let Some(overlay) = app.overlay.take() else { return };
    let overlay = match overlay {
        Overlay::Help { scroll } => {
            let s = help(f, area, app, &theme, scroll);
            Overlay::Help { scroll: s }
        }
        Overlay::Search(mut s) => {
            search(f, area, app, &theme, &mut s);
            Overlay::Search(s)
        }
        Overlay::Prompt(p) => {
            let inner = panel(f, area, 60, if p.error.is_some() { 8 } else { 7 }, p.title, &theme);
            let buf = f.buffer_mut();
            input_line(&p.input, inner.width as usize, &theme, "› ").render(line_at(inner, 1), buf);
            if let Some(e) = &p.error {
                Line::styled(truncate(e, inner.width as usize), theme.error()).render(line_at(inner, 3), buf);
            }
            let hint = match p.purpose {
                crate::app::PromptPurpose::AddFolder => "ENTER add   TAB complete   ESC cancel",
                _ => "ENTER save   ESC cancel",
            };
            Line::styled(hint, theme.muted()).render(line_at(inner, inner.height.saturating_sub(1)), buf);
            Overlay::Prompt(p)
        }
        Overlay::PlaylistPicker { mut cursor, tracks } => {
            let n = app.state.playlists.len() + 1;
            let h = (n as u16 + 6).min(area.height.saturating_sub(2)).max(7);
            let title = format!("ADD {} TO", plural(tracks.len(), "TRACK", "TRACKS"));
            let inner = panel(f, area, 54, h, &title, &theme);
            let list = split_v(inner, &[1, 0, 1, 1]);
            let buf = f.buffer_mut();
            let range = cursor.scroll_into_view(n, list[1].height as usize);
            for (row, i) in range.enumerate() {
                let r = line_at(list[1], row as u16);
                let selected = i == cursor.selected;
                let base = row_style(&theme, selected, true);
                buf.set_style(r, base);
                if i == 0 {
                    Line::styled("  + New playlist…", base.patch(theme.accent())).render(r, buf);
                } else {
                    let p = &app.state.playlists[i - 1];
                    let count = plural(p.tracks.len(), "track", "tracks");
                    let w = r.width as usize;
                    Line::from(vec![
                        Span::styled("  ", base),
                        Span::styled(fit(&p.name, w.saturating_sub(14)), base),
                        Span::styled(fit_right(&count, 12), base.patch(theme.muted())),
                    ])
                    .render(r, buf);
                }
            }
            Line::styled("ENTER choose   ESC cancel", theme.muted()).render(list[3], buf);
            Overlay::PlaylistPicker { cursor, tracks }
        }
        Overlay::Confirm { message, action } => {
            let w = (width(&message) as u16 + 8).clamp(30, 70);
            let inner = panel(f, area, w, 6, "CONFIRM", &theme);
            let buf = f.buffer_mut();
            Line::styled(truncate(&message, inner.width as usize), theme.strong())
                .render(line_at(inner, 1), buf);
            Line::from(vec![
                Span::styled("y", theme.strong()),
                Span::styled(" yes   ", theme.muted()),
                Span::styled("n", theme.strong()),
                Span::styled(" no", theme.muted()),
            ])
            .render(line_at(inner, 3), buf);
            Overlay::Confirm { message, action }
        }
    };
    app.overlay = Some(overlay);
}

fn help(f: &mut Frame, area: Rect, app: &App, theme: &Theme, scroll: usize) -> usize {
    let groups = [Group::Playback, Group::Navigation, Group::Library, Group::App];
    let mut lines: Vec<Line> = Vec::new();
    let key_w = 16;
    for g in groups {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(g.label(), theme.label()));
        for b in app.keymap.bindings().iter().filter(|b| b.group == g) {
            let keys = b.keys.iter().map(|k| k.label()).collect::<Vec<_>>().join(" ");
            lines.push(Line::from(vec![
                Span::styled(fit(&keys, key_w), theme.strong()),
                Span::styled(b.help, theme.muted()),
            ]));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("In text fields every key types; ESC leaves the field.", theme.faint()));
    let h = (lines.len() as u16 + 2).min(area.height);
    let inner = panel(f, area, 64, h, "KEYBOARD", theme);
    let max_scroll = lines.len().saturating_sub(inner.height as usize);
    let scroll = scroll.min(max_scroll);
    let buf = f.buffer_mut();
    for (row, line) in lines.into_iter().skip(scroll).take(inner.height as usize).enumerate() {
        line.render(line_at(inner, row as u16), buf);
    }
    scroll
}

fn search(f: &mut Frame, area: Rect, app: &App, theme: &Theme, s: &mut crate::app::SearchState) {
    let w = area.width.saturating_sub(4).min(110);
    let h = area.height.saturating_sub(2);
    let inner = panel(f, area, w, h, "SEARCH", theme);
    let rows = split_v(inner, &[1, 1, 1, 0]);
    let buf = f.buffer_mut();
    let prompt = if s.results_focused { "  " } else { "/ " };
    if s.results_focused {
        Line::from(vec![
            Span::styled("/ ", theme.muted()),
            Span::styled(s.input.value.clone(), theme.text()),
        ])
        .render(rows[0], buf);
    } else {
        input_line(&s.input, rows[0].width as usize, theme, prompt).render(rows[0], buf);
    }
    let count = if s.input.value.trim().is_empty() {
        "track · artist · album".to_string()
    } else {
        plural(s.results.len(), "result", "results")
    };
    let cw = width(&count) as u16;
    if cw < rows[0].width {
        Line::styled(count, theme.muted())
            .render(Rect { x: rows[0].x + rows[0].width - cw, width: cw, ..rows[0] }, buf);
    }
    hrule(buf, rows[1], theme);
    let list = rows[3];
    if s.results.is_empty() {
        let msg = if s.input.value.trim().is_empty() { "Type to search your library" } else { "No matches" };
        center_line(buf, line_at(list, list.height / 3), Line::styled(msg, theme.muted()));
        return;
    }
    let lw = list.width as usize;
    let time_w = 7;
    let flex = lw.saturating_sub(time_w + 3);
    let (tw, aw, alw) = if lw >= 80 {
        (flex * 42 / 100, flex * 30 / 100, flex - flex * 42 / 100 - flex * 30 / 100)
    } else {
        (flex * 60 / 100, flex - flex * 60 / 100, 0)
    };
    let range = s.cursor.scroll_into_view(s.results.len(), list.height as usize);
    for (row, i) in range.enumerate() {
        let Some(t) = app.track(s.results[i]) else { continue };
        let selected = i == s.cursor.selected;
        let base = row_style(theme, selected, s.results_focused || selected);
        let r = line_at(list, row as u16);
        buf.set_style(r, base);
        let fav = app.state.is_favorite(&t.path);
        let mut spans = vec![
            Span::styled(if fav { "♥ " } else { "  " }, base.patch(theme.accent())),
            Span::styled(fit(&t.title, tw), base),
            Span::styled(fit(&t.artist, aw), base.patch(theme.muted())),
        ];
        if alw > 0 {
            spans.push(Span::styled(fit(&t.album, alw), base.patch(theme.muted())));
        }
        spans.push(Span::styled(fit_right(&format_duration(t.duration), time_w), base.patch(theme.muted())));
        Line::from(spans).render(r, buf);
    }
}

// ───────────────────────────── welcome ─────────────────────────────

const WORDMARK: [&str; 3] =
    ["▀█▀ █▀▀ █▀█ █▀▄▀█ █ █▄ █ ▄▀█ █     █▀▀ █▀▄▀█", " █  ██▄ █▀▄ █ ▀ █ █ █ ▀█ █▀█ █▄▄ ▄ █▀  █ ▀ █", ""];

pub fn welcome(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let Phase::Welcome(w) = &app.phase else { return };
    let buf = f.buffer_mut();
    let col_w = area.width.saturating_sub(4).min(64);
    let content_h = 15u16;
    let top = area.y + area.height.saturating_sub(content_h) / 2;
    let col = Rect {
        x: area.x + (area.width - col_w) / 2,
        y: top,
        width: col_w,
        height: area.height.saturating_sub(top - area.y),
    };
    let mut y = 0u16;

    if col_w as usize >= width(WORDMARK[0]) && area.height >= 20 {
        for (i, l) in WORDMARK.iter().take(2).enumerate() {
            let style = if i == 0 { theme.strong() } else { theme.accent() };
            center_line(buf, line_at(col, y), Line::styled(*l, style));
            y += 1;
        }
    } else {
        center_line(
            buf,
            line_at(col, y),
            Line::from(vec![
                Span::styled("TERMINAL", theme.strong()),
                Span::styled(".FM", theme.accent().add_modifier(Modifier::BOLD)),
            ]),
        );
        y += 1;
    }
    y += 1;
    center_line(buf, line_at(col, y), Line::styled("Your terminal. Your music. Your vibe.", theme.muted()));
    y += 2;
    hrule(buf, line_at(col, y), &theme);
    y += 2;

    if let Some(job) = &app.scan {
        let folder = app.config.music_dirs.first().map(|p| p.display().to_string()).unwrap_or_default();
        Line::styled("Building your library", theme.strong()).render(line_at(col, y), buf);
        y += 1;
        Line::styled(truncate(&folder, col_w as usize), theme.muted()).render(line_at(col, y), buf);
        y += 2;
        let (done, total) = (job.done, job.discovered.unwrap_or(0));
        let ratio = if total == 0 { 0.0 } else { done as f32 / total as f32 };
        let label = match job.discovered {
            None => "looking for audio files…".to_string(),
            Some(t) => format!("{} / {} files", thousands(done), thousands(t)),
        };
        let bar_w = (col_w as usize).saturating_sub(width(&label) + 2);
        let mut spans = progress_spans(&theme, ratio, bar_w);
        spans.push(Span::styled(format!("  {label}"), theme.muted()));
        Line::from(spans).render(line_at(col, y), buf);
        y += 2;
        Line::styled("ESC quit", theme.faint()).render(line_at(col, y), buf);
        return;
    }

    Line::styled("Where is your music?", theme.strong()).render(line_at(col, y), buf);
    y += 2;
    input_line(&w.input, col_w as usize, &theme, "› ").render(line_at(col, y), buf);
    y += 1;
    if let Some(e) = &w.error {
        Line::styled(truncate(e, col_w as usize), theme.error()).render(line_at(col, y), buf);
    }
    y += 2;
    Line::from(vec![
        Span::styled("ENTER", theme.strong()),
        Span::styled(" scan   ", theme.muted()),
        Span::styled("TAB", theme.strong()),
        Span::styled(" complete   ", theme.muted()),
        Span::styled("ESC", theme.strong()),
        Span::styled(" quit", theme.muted()),
    ])
    .render(line_at(col, y), buf);
    y += 2;
    Line::styled(
        truncate("Everything stays on this machine. No accounts, no network.", col_w as usize),
        theme.muted(),
    )
    .render(line_at(col, y), buf);
}
