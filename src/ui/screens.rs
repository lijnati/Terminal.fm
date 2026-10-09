//! Main screens.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::text::{fit, fit_right, plural, truncate};
use super::theme::Theme;
use super::{
    Density, center_line, hrule, line_at, line_lr, play_glyph, progress_spans, row_style, split_v, timing,
    viz,
};
use crate::analysis::Signal;
use crate::app::{App, LibTab, ListCursor, SettingRow};
use crate::audio::PlayState;
use crate::library::{Track, TrackIdx, format_duration};
use crate::playback::Repeat;
use crate::storage::{TrackRef, VisualizerMode};

// ───────────────────────────── shared list rendering ─────────────────────────────

/// Which optional columns fit at a given width.
struct Columns {
    num: usize,
    title: usize,
    artist: usize,
    album: usize,
    time: usize,
}

impl Columns {
    fn for_width(w: usize, numbered: bool) -> Self {
        let num = if numbered && w >= 50 { 4 } else { 0 };
        let time = if w >= 30 { 7 } else { 0 };
        let marker = 3; // "▶ " + favorite mark
        let flex = w.saturating_sub(num + time + marker);
        let (title, artist, album) = if w >= 100 {
            (flex * 42 / 100, flex * 30 / 100, flex - flex * 42 / 100 - flex * 30 / 100)
        } else if w >= 56 {
            (flex * 58 / 100, flex - flex * 58 / 100, 0)
        } else {
            (flex, 0, 0)
        };
        Columns { num, title, artist, album, time }
    }
}

fn column_header(buf: &mut Buffer, area: Rect, theme: &Theme, cols: &Columns, first: &str) {
    let mut s = vec![Span::raw("  ")];
    if cols.num > 0 {
        s.push(Span::styled(fit("#", cols.num), theme.label()));
    }
    s.push(Span::styled(fit(first, cols.title), theme.label()));
    s.push(Span::raw(" "));
    if cols.artist > 0 {
        s.push(Span::styled(fit("ARTIST", cols.artist), theme.label()));
    }
    if cols.album > 0 {
        s.push(Span::styled(fit("ALBUM", cols.album), theme.label()));
    }
    if cols.time > 0 {
        s.push(Span::styled(fit_right("TIME", cols.time), theme.label()));
    }
    Line::from(s).render(area, buf);
}

/// A track row. `None` track renders a missing-file placeholder from the stored reference.
#[allow(clippy::too_many_arguments)]
fn track_row(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    cols: &Columns,
    number: usize,
    idx: Option<TrackIdx>,
    fallback: Option<&TrackRef>,
    selected: bool,
    focused: bool,
) {
    let theme = &app.theme;
    let base = row_style(theme, selected, focused);
    buf.set_style(area, base);
    let track: Option<&Track> = idx.and_then(|i| app.track(i));
    let playing = idx.is_some() && app.now.as_ref().map(|n| n.idx) == idx;
    let (title, artist, album, dur, missing) = match (track, fallback) {
        (Some(t), _) => {
            (t.title.as_str(), t.artist.as_str(), t.album.as_str(), format_duration(t.duration), false)
        }
        (None, Some(r)) => {
            (r.title.as_str(), r.artist.as_str(), r.album.as_str(), "missing".to_string(), true)
        }
        (None, None) => ("?", "", "", String::new(), true),
    };
    let fav = track.is_some_and(|t| app.state.is_favorite(&t.path));
    let dim = |s: ratatui::style::Style| {
        if missing { theme.faint().patch(base.remove_modifier(Modifier::BOLD)).fg(theme.faint) } else { s }
    };
    let marker = if playing { play_glyph(app.play_state()).chars().next().unwrap_or(' ') } else { ' ' };
    let mut s = vec![Span::styled(format!("{marker} "), base.patch(theme.accent()))];
    if cols.num > 0 {
        s.push(Span::styled(fit(&format!("{number:02}"), cols.num), dim(base.patch(theme.muted()))));
    }
    let title_style = if playing { base.patch(theme.accent()) } else { base };
    s.push(Span::styled(fit(title, cols.title), dim(title_style)));
    s.push(Span::styled(if fav { "♥" } else { " " }, base.patch(theme.accent())));
    if cols.artist > 0 {
        s.push(Span::styled(fit(artist, cols.artist), dim(base.patch(theme.muted()))));
    }
    if cols.album > 0 {
        s.push(Span::styled(fit(album, cols.album), dim(base.patch(theme.muted()))));
    }
    if cols.time > 0 {
        s.push(Span::styled(fit_right(&dur, cols.time), dim(base.patch(theme.muted()))));
    }
    Line::from(s).render(area, buf);
}

fn empty_state(buf: &mut Buffer, area: Rect, theme: &Theme, title: &str, hint: &str) {
    if area.height == 0 {
        return;
    }
    let y = area.height / 3;
    center_line(buf, line_at(area, y), Line::styled(truncate(title, area.width as usize), theme.strong()));
    center_line(buf, line_at(area, y + 2), Line::styled(truncate(hint, area.width as usize), theme.muted()));
}

/// Section label with a right-aligned detail, e.g. `LIBRARY ······ 1,204 tracks`.
fn section(buf: &mut Buffer, area: Rect, theme: &Theme, label: &str, detail: &str) {
    line_lr(
        buf,
        area,
        Line::styled(label.to_string(), theme.label()),
        Line::styled(detail.to_string(), theme.muted()),
    );
}

/// Renders a list of track references (favorites, recent, playlist detail).
fn ref_list(
    buf: &mut Buffer,
    area: Rect,
    app: &App,
    refs: &[TrackRef],
    cursor: &mut ListCursor,
    focused: bool,
) {
    let rows = split_v(area, &[1, 0]);
    let list = rows[1];
    let content = rows_area(list, refs.len());
    let cols = Columns::for_width(content.width as usize, true);
    column_header(buf, Rect { width: content.width, ..rows[0] }, &app.theme, &cols, "TITLE");
    let range = cursor.scroll_into_view(refs.len(), list.height as usize);
    for (row, i) in range.enumerate() {
        let r = &refs[i];
        track_row(
            buf,
            line_at(content, row as u16),
            app,
            &cols,
            i + 1,
            app.library.find(&r.path),
            Some(r),
            i == cursor.selected,
            focused,
        );
    }
    scrollbar(buf, list, &app.theme, cursor.offset, list.height as usize, refs.len());
}

/// Leaves the rightmost column free for the scrollbar when `total` rows overflow.
fn rows_area(list: Rect, total: usize) -> Rect {
    if total > list.height as usize && list.width > 1 { Rect { width: list.width - 2, ..list } } else { list }
}

/// Thin scroll indicator in the rightmost column when content overflows.
fn scrollbar(buf: &mut Buffer, area: Rect, theme: &Theme, offset: usize, visible: usize, total: usize) {
    if total <= visible || area.height == 0 || area.width == 0 {
        return;
    }
    let h = area.height as usize;
    let thumb = (h * visible / total).max(1);
    let start = ((h - thumb) * offset) / (total - visible).max(1);
    let x = area.x + area.width - 1;
    for i in 0..h {
        let ch = if i >= start && i < start + thumb { '┃' } else { '│' };
        let style = if ch == '┃' { theme.muted() } else { theme.faint() };
        buf[(x, area.y + i as u16)].set_char(ch).set_style(style);
    }
}

// ───────────────────────────── now playing ─────────────────────────────

pub fn now_playing(f: &mut Frame, area: Rect, app: &mut App, density: Density) {
    let theme = app.theme.clone();
    let (main, queue_area) = match density {
        Density::Full => {
            let qw = (area.width / 3).clamp(32, 52);
            let main = Rect { width: area.width.saturating_sub(qw + 3), ..area };
            let sep_x = main.x + main.width + 1;
            for y in area.y..area.y + area.height {
                f.buffer_mut()[(sep_x, y)].set_char('│').set_style(theme.faint());
            }
            (main, Some(Rect { x: sep_x + 2, width: qw, ..area }))
        }
        Density::Standard if area.height >= 22 => {
            let qh = (area.height / 4).clamp(4, 9);
            let rows = split_v(area, &[0, 1, qh]);
            hrule(f.buffer_mut(), rows[1], &theme);
            (rows[0], Some(rows[2]))
        }
        _ => (area, None),
    };
    now_playing_main(f, main, app, density);
    if let Some(q) = queue_area {
        queue_panel(f.buffer_mut(), q, app);
    }
}

fn now_playing_main(f: &mut Frame, area: Rect, app: &mut App, density: Density) {
    let theme = app.theme.clone();
    let compact = density == Density::Compact;
    // Fixed rows: label, gap, title, meta, gap, [viz], gap, progress, gap, transport, gap, flags.
    let rows = if compact {
        split_v(area, &[1, 1, 1, 0, 1, 1])
    } else {
        split_v(area, &[1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1])
    };
    let buf = f.buffer_mut();
    let track = app.current_track().cloned();
    let state = app.play_state();

    let (label_r, title_r, meta_r, viz_r, progress_r, transport_r, flags_r) = if compact {
        (rows[0], rows[1], rows[2], rows[3], rows[4], Rect::default(), rows[5])
    } else {
        (rows[0], rows[2], rows[3], rows[5], rows[7], rows[9], rows[11])
    };

    let status_label = match state {
        PlayState::Playing => "NOW PLAYING",
        PlayState::Paused => "PAUSED",
        PlayState::Stopped => "STOPPED",
    };
    let pos_in_queue = match app.queue.position() {
        Some(p) => format!("{} of {}", p + 1, app.queue.len()),
        None => String::new(),
    };
    section(buf, label_r, &theme, if compact { "" } else { status_label }, &pos_in_queue);

    match &track {
        Some(t) => {
            let fav = app.state.is_favorite(&t.path);
            let mut title = vec![Span::styled(
                truncate(&t.title, title_r.width.saturating_sub(3) as usize),
                theme.strong(),
            )];
            if fav {
                title.push(Span::styled("  ♥", theme.accent()));
            }
            Line::from(title).render(title_r, buf);
            let mut meta = vec![t.artist.clone(), t.album.clone()];
            if let Some(y) = t.year {
                meta.push(y.to_string());
            }
            Line::styled(truncate(&meta.join(" · "), meta_r.width as usize), theme.muted())
                .render(meta_r, buf);
        }
        None => {
            Line::styled("Nothing playing", theme.strong()).render(title_r, buf);
            let hint = if app.library.is_empty() {
                "Add a music folder in settings (6) to get started"
            } else {
                "Browse your library (2) or search (/) — Enter plays"
            };
            Line::styled(truncate(hint, meta_r.width as usize), theme.muted()).render(meta_r, buf);
        }
    }

    visualizer(buf, viz_r, app, state, track.is_some());

    // Progress.
    let (pos, dur, ratio) = timing(app);
    let (lt, rt) = (format_duration(pos), format_duration(dur));
    let bar_w = (progress_r.width as usize).saturating_sub(lt.len() + rt.len() + 4);
    let mut p = vec![Span::styled(format!("{lt}  "), theme.text())];
    if track.is_some() {
        p.extend(progress_spans(&theme, ratio, bar_w));
    } else {
        p.push(Span::styled("─".repeat(bar_w), theme.faint()));
    }
    p.push(Span::styled(format!("  {rt}"), theme.muted()));
    Line::from(p).render(progress_r, buf);

    // Transport.
    if transport_r.height > 0 {
        let main = match state {
            PlayState::Playing => "❚❚",
            _ => "▶",
        };
        let t = Line::from(vec![
            Span::styled("◀◀", theme.muted()),
            Span::raw("       "),
            Span::styled(main, theme.accent().add_modifier(Modifier::BOLD)),
            Span::raw("       "),
            Span::styled("▶▶", theme.muted()),
        ]);
        center_line(buf, transport_r, t);
    }

    flags(buf, flags_r, app, &theme, compact);
}

fn flags(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme, compact: bool) {
    let on = |b: bool| if b { theme.accent() } else { theme.text() };
    let shuffle = app.queue.shuffle();
    let repeat = app.queue.repeat();
    let vol = if app.muted { "MUTED".to_string() } else { format!("{}%", app.volume) };
    let items: Vec<(String, String, bool)> = vec![
        ("SHUFFLE".into(), if shuffle { "ON" } else { "OFF" }.into(), shuffle),
        ("REPEAT".into(), repeat.label().into(), repeat != Repeat::Off),
        ("VOL".into(), vol, false),
        ("VIZ".into(), app.visualizer.label().into(), false),
    ];
    let mut spans = Vec::new();
    let w = area.width as usize;
    // Spread evenly when wide; compact labels when narrow.
    let total: usize = items.iter().map(|(l, v, _)| l.len() + 1 + v.len()).sum();
    let gap = if compact || w < total + 6 { 2 } else { ((w - total) / items.len().max(1)).min(8) };
    for (i, (label, value, active)) in items.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(gap)));
        }
        spans.push(Span::styled(format!("{label} "), theme.label()));
        spans.push(Span::styled(value, on(active)));
    }
    Line::from(spans).render(area, buf);
}

fn visualizer(buf: &mut Buffer, area: Rect, app: &mut App, state: PlayState, has_track: bool) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let theme = app.theme.clone();
    let area = Rect { y: area.y, height: area.height, ..area };
    let signal = app.viz.analyzer.signal();
    let peaks = app.config.visualizer.peaks;
    let requested = app.config.visualizer.bars;
    match app.visualizer {
        VisualizerMode::Off => {
            center_line(
                buf,
                line_at(area, area.height / 2),
                Line::styled("visualizer off · v to change", theme.faint()),
            );
            return;
        }
        VisualizerMode::Spectrum => {
            let (n, ..) = viz::spectrum_layout(area.width, requested);
            app.viz.bands = n;
            viz::spectrum(buf, area, app.viz.analyzer.frame(), &theme, peaks, requested);
            // Baseline so the quiet state still has structure.
            let y = area.y + area.height - 1;
            for x in area.x..area.x + area.width {
                if buf[(x, y)].symbol() == " " {
                    buf[(x, y)].set_char('▁').set_style(theme.faint());
                }
            }
        }
        VisualizerMode::Waveform => {
            let points = area.width as usize * 2;
            let dt = app.viz.dt;
            let sens = app.config.visualizer.sensitivity;
            let wave = app.viz.analyzer.waveform(points, dt, sens).to_vec();
            viz::waveform(buf, area, &wave, &theme);
        }
        VisualizerMode::Minimal => viz::minimal(buf, area, app.viz.analyzer.frame(), &theme),
    }
    // Honest state labels: never pretend there is signal.
    let label = match (state, signal, has_track) {
        (PlayState::Playing, Signal::None, true) => Some(("NO SIGNAL", theme.warn())),
        (PlayState::Playing, Signal::Silence, true) => Some(("SILENCE", theme.muted())),
        (PlayState::Paused, _, _) => Some(("PAUSED", theme.muted())),
        _ => None,
    };
    if let Some((text, style)) = label {
        if area.height >= 2 {
            let w = text.len() as u16 + 2;
            if area.width > w {
                Line::styled(format!(" {text} "), style)
                    .render(Rect { x: area.x + area.width - w, y: area.y, width: w, height: 1 }, buf);
            }
        }
    }
}

fn queue_panel(buf: &mut Buffer, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let view = app.queue_view();
    let rows = split_v(area, &[1, 1, 0]);
    let detail = if view.is_empty() { String::new() } else { plural(view.len(), "track", "tracks") };
    let label = if app.queue_focused { "QUEUE ◂" } else { "QUEUE" };
    section(buf, rows[0], &theme, label, &detail);
    let list = rows[2];
    if view.is_empty() {
        Line::styled(truncate("Queue is empty — press a on any track", list.width as usize), theme.faint())
            .render(line_at(list, 0), buf);
        return;
    }
    let mut cursor = app.queue_cursor;
    let range = cursor.scroll_into_view(view.len(), list.height as usize);
    let w = list.width as usize;
    let time_w = 6;
    let num_w = 4;
    let rest = w.saturating_sub(time_w + num_w + 1);
    let (title_w, artist_w) = if w >= 44 { (rest * 60 / 100, rest - rest * 60 / 100) } else { (rest, 0) };
    for (row, i) in range.enumerate() {
        let (_, idx) = view[i];
        let Some(t) = app.track(idx) else { continue };
        let selected = app.queue_focused && i == cursor.selected;
        let current = i == 0 && app.queue.current().is_some();
        let base = row_style(&theme, selected, true);
        let r = line_at(list, row as u16);
        buf.set_style(r, base);
        let num = if current {
            format!("{} ", play_glyph(app.play_state()).chars().next().unwrap_or('▶'))
        } else {
            format!("{:02}", i)
        };
        let mut s = vec![
            Span::styled(fit(&num, num_w), base.patch(if current { theme.accent() } else { theme.faint() })),
            Span::styled(fit(&t.title, title_w), if current { base.patch(theme.accent()) } else { base }),
        ];
        if artist_w > 0 {
            s.push(Span::styled(fit(&t.artist, artist_w), base.patch(theme.muted())));
        }
        s.push(Span::styled(fit_right(&format_duration(t.duration), time_w), base.patch(theme.muted())));
        Line::from(s).render(r, buf);
    }
    app.queue_cursor = cursor;
}

// ───────────────────────────── library ─────────────────────────────

pub fn library(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let buf = f.buffer_mut();
    let rows = split_v(area, &[1, 1, 0]);
    // Tabs.
    let mut tabs = Vec::new();
    for t in LibTab::ALL {
        let active = t == app.lib.tab();
        tabs.push(Span::styled(
            t.label(),
            if active { theme.strong().add_modifier(Modifier::UNDERLINED) } else { theme.muted() },
        ));
        tabs.push(Span::raw("   "));
    }
    let detail = match (app.lib.tab(), app.lib.open) {
        (LibTab::Tracks, _) => {
            format!("{} · by {}", plural(app.library.len(), "track", "tracks"), app.lib.sort.label())
        }
        (_, Some(g)) => app.lib.groups.get(g).map(|g| format!("‹ {}", g.name)).unwrap_or_default(),
        (t, None) => plural(
            app.lib.groups.len(),
            &t.label().to_lowercase()[..t.label().len() - 1],
            &t.label().to_lowercase(),
        ),
    };
    line_lr(
        buf,
        rows[0],
        Line::from(tabs),
        Line::styled(truncate(&detail, (rows[0].width / 2) as usize), theme.muted()),
    );
    let body = rows[2];

    if app.library.is_empty() {
        let (t, h) = if app.scan.is_some() {
            ("Scanning your music…", "Tracks will appear here as soon as the scan finishes")
        } else {
            ("Your library is empty", "Add a folder in settings (6), or run: terminalfm ~/Music")
        };
        empty_state(buf, body, &theme, t, h);
        return;
    }
    if app.lib.tab() == LibTab::Genres && app.lib.groups.is_empty() {
        empty_state(buf, body, &theme, "No genre tags found", "Genres come from your files' metadata");
        return;
    }

    if app.lib.showing_groups() {
        group_list(buf, body, app);
    } else {
        let parts = split_v(body, &[1, 0]);
        let list = parts[1];
        let content = rows_area(list, app.lib.tracks.len());
        let cols = Columns::for_width(content.width as usize, app.lib.open.is_some());
        column_header(buf, Rect { width: content.width, ..parts[0] }, &theme, &cols, "TITLE");
        let mut cursor = app.lib.cursor;
        let range = cursor.scroll_into_view(app.lib.tracks.len(), list.height as usize);
        for (row, i) in range.enumerate() {
            let idx = app.lib.tracks[i];
            let num = app.track(idx).and_then(|t| t.track_no).map(|n| n as usize).unwrap_or(i + 1);
            track_row(
                buf,
                line_at(content, row as u16),
                app,
                &cols,
                num,
                Some(idx),
                None,
                i == cursor.selected,
                true,
            );
        }
        scrollbar(buf, list, &theme, cursor.offset, list.height as usize, app.lib.tracks.len());
        app.lib.cursor = cursor;
    }
}

fn group_list(buf: &mut Buffer, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let full = area;
    let area = rows_area(Rect { height: area.height.saturating_sub(1), ..area }, app.lib.groups.len());
    let area = Rect { height: full.height, ..area };
    let w = area.width as usize;
    let count_w = 12;
    let time_w = if w >= 50 { 9 } else { 0 };
    let detail_w =
        if app.lib.tab() == LibTab::Albums && w >= 70 { (w - count_w - time_w) * 40 / 100 } else { 0 };
    let name_w = w.saturating_sub(count_w + time_w + detail_w + 2);
    let parts = split_v(area, &[1, 0]);
    let mut header = vec![
        Span::raw("  "),
        Span::styled(fit(&app.lib.tab().label()[..app.lib.tab().label().len() - 1], name_w), theme.label()),
    ];
    if detail_w > 0 {
        header.push(Span::styled(fit("ARTIST", detail_w), theme.label()));
    }
    header.push(Span::styled(fit_right("TRACKS", count_w), theme.label()));
    if time_w > 0 {
        header.push(Span::styled(fit_right("LENGTH", time_w), theme.label()));
    }
    Line::from(header).render(parts[0], buf);
    let list = parts[1];
    let mut cursor = app.lib.cursor;
    let range = cursor.scroll_into_view(app.lib.groups.len(), list.height as usize);
    let playing = app.now.as_ref().map(|n| n.idx);
    for (row, i) in range.enumerate() {
        let g = &app.lib.groups[i];
        let selected = i == cursor.selected;
        let base = row_style(&theme, selected, true);
        let r = line_at(list, row as u16);
        buf.set_style(r, base);
        let has_playing = playing.is_some_and(|p| g.tracks.contains(&p));
        let total: std::time::Duration =
            g.tracks.iter().filter_map(|&t| app.track(t)).map(|t| t.duration).sum();
        let mut s = vec![
            Span::styled(if has_playing { "▸ " } else { "  " }, base.patch(theme.accent())),
            Span::styled(fit(&g.name, name_w), base),
        ];
        if detail_w > 0 {
            s.push(Span::styled(fit(&g.detail, detail_w), base.patch(theme.muted())));
        }
        s.push(Span::styled(fit_right(&g.tracks.len().to_string(), count_w), base.patch(theme.muted())));
        if time_w > 0 {
            s.push(Span::styled(fit_right(&format_duration(total), time_w), base.patch(theme.muted())));
        }
        Line::from(s).render(r, buf);
    }
    let list = Rect { width: full.width, ..list };
    scrollbar(buf, list, &theme, cursor.offset, list.height as usize, app.lib.groups.len());
    app.lib.cursor = cursor;
}

// ───────────────────────────── playlists / favorites / recent ─────────────────────────────

pub fn playlists(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let buf = f.buffer_mut();
    let rows = split_v(area, &[1, 1, 0]);
    if let Some(p) = app.playlists.open {
        let Some(pl) = app.state.playlists.get(p).cloned() else { return };
        let total: std::time::Duration = pl
            .tracks
            .iter()
            .filter_map(|r| app.library.find(&r.path))
            .filter_map(|i| app.track(i))
            .map(|t| t.duration)
            .sum();
        section(
            buf,
            rows[0],
            &theme,
            &format!("PLAYLISTS ‹ {}", pl.name.to_uppercase()),
            &format!("{} · {}", plural(pl.tracks.len(), "track", "tracks"), format_duration(total)),
        );
        if pl.tracks.is_empty() {
            empty_state(
                buf,
                rows[2],
                &theme,
                "This playlist is empty",
                "Press A on any track to add it here",
            );
            return;
        }
        let mut cursor = app.playlists.track_cursor;
        ref_list(buf, rows[2], app, &pl.tracks, &mut cursor, true);
        app.playlists.track_cursor = cursor;
        return;
    }
    section(buf, rows[0], &theme, "PLAYLISTS", &plural(app.state.playlists.len(), "playlist", "playlists"));
    if app.state.playlists.is_empty() {
        empty_state(buf, rows[2], &theme, "No playlists yet", "Press c to create one, or A on any track");
        return;
    }
    let list = rows[2];
    let w = list.width as usize;
    let mut cursor = app.playlists.cursor;
    let range = cursor.scroll_into_view(app.state.playlists.len(), list.height as usize);
    for (row, i) in range.enumerate() {
        let pl = &app.state.playlists[i];
        let selected = i == cursor.selected;
        let base = row_style(&theme, selected, true);
        let r = line_at(list, row as u16);
        buf.set_style(r, base);
        let resolved: Vec<_> = pl.tracks.iter().filter_map(|t| app.library.find(&t.path)).collect();
        let total: std::time::Duration =
            resolved.iter().filter_map(|&i| app.track(i)).map(|t| t.duration).sum();
        let missing = pl.tracks.len() - resolved.len();
        let count = if missing > 0 {
            format!("{} · {missing} missing", plural(pl.tracks.len(), "track", "tracks"))
        } else {
            plural(pl.tracks.len(), "track", "tracks")
        };
        let right_w = 30.min(w / 2);
        let s = vec![
            Span::styled("  ", base),
            Span::styled(fit(&pl.name, w.saturating_sub(right_w + 11)), base),
            Span::styled(fit_right(&count, right_w), base.patch(theme.muted())),
            Span::styled(fit_right(&format_duration(total), 9), base.patch(theme.muted())),
        ];
        Line::from(s).render(r, buf);
    }
    app.playlists.cursor = cursor;
}

pub fn favorites(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let buf = f.buffer_mut();
    let rows = split_v(area, &[1, 1, 0]);
    let refs = app.state.favorites.clone();
    section(buf, rows[0], &theme, "FAVORITES", &plural(refs.len(), "track", "tracks"));
    if refs.is_empty() {
        empty_state(
            buf,
            rows[2],
            &theme,
            "No favorites yet",
            "Press f on any track — or on the one playing now",
        );
        return;
    }
    let mut cursor = app.favorites_cursor;
    ref_list(buf, rows[2], app, &refs, &mut cursor, true);
    app.favorites_cursor = cursor;
}

pub fn recent(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let buf = f.buffer_mut();
    let rows = split_v(area, &[1, 1, 0]);
    let refs = app.state.recent.clone();
    section(buf, rows[0], &theme, "RECENTLY PLAYED", &plural(refs.len(), "track", "tracks"));
    if refs.is_empty() {
        empty_state(
            buf,
            rows[2],
            &theme,
            "Nothing played yet",
            "Tracks you play show up here, most recent first",
        );
        return;
    }
    let mut cursor = app.recent_cursor;
    ref_list(buf, rows[2], app, &refs, &mut cursor, true);
    app.recent_cursor = cursor;
}

// ───────────────────────────── settings ─────────────────────────────

fn setting_text(app: &App, row: SettingRow) -> (String, String) {
    let v = &app.config.visualizer;
    match row {
        SettingRow::Theme => ("Theme".into(), app.config.theme.label().into()),
        SettingRow::Visualizer => ("Visualizer".into(), app.visualizer.label().to_lowercase()),
        SettingRow::Sensitivity => ("Sensitivity".into(), format!("{:.2}×", v.sensitivity)),
        SettingRow::Fps => ("Refresh rate".into(), format!("{} fps", v.fps)),
        SettingRow::Peaks => ("Spectrum peaks".into(), if v.peaks { "on" } else { "off" }.into()),
        SettingRow::Volume => ("Startup volume".into(), format!("{}%", app.config.volume)),
        SettingRow::SeekStep => ("Seek step".into(), format!("{} s", app.config.seek_seconds)),
        SettingRow::Folder(i) => (
            "Folder".into(),
            app.config.music_dirs.get(i).map(|p| p.display().to_string()).unwrap_or_default(),
        ),
        SettingRow::AddFolder => ("".into(), "+ Add music folder…".into()),
        SettingRow::Rescan => {
            ("".into(), if app.scan.is_some() { "Scanning…".into() } else { "↻ Rescan library".into() })
        }
    }
}

fn setting_section(row: SettingRow) -> &'static str {
    match row {
        SettingRow::Theme
        | SettingRow::Visualizer
        | SettingRow::Sensitivity
        | SettingRow::Fps
        | SettingRow::Peaks => "APPEARANCE",
        SettingRow::Volume | SettingRow::SeekStep => "PLAYBACK",
        _ => "LIBRARY",
    }
}

pub fn settings(f: &mut Frame, area: Rect, app: &mut App) {
    let theme = app.theme.clone();
    let buf = f.buffer_mut();
    let rows = app.setting_rows();
    // Display lines: section headers interleaved with rows (None = header/gap).
    let mut lines: Vec<(Option<usize>, &'static str)> = Vec::new();
    let mut last = "";
    for (i, r) in rows.iter().enumerate() {
        let s = setting_section(*r);
        if s != last {
            if !lines.is_empty() {
                lines.push((None, ""));
            }
            lines.push((None, s));
            last = s;
        }
        lines.push((Some(i), ""));
    }
    let mut footer_note = String::new();
    if let Some(p) = &app.paths {
        footer_note = format!("config · {}", p.config_file.display());
    }
    if !app.config_writable {
        footer_note = "config.toml has errors — changes apply to this session only".into();
    }
    let parts = split_v(area, &[0, 1]);
    let list = parts[0];
    let selected_line = lines.iter().position(|(r, _)| *r == Some(app.settings_cursor.selected)).unwrap_or(0);
    let mut lc = ListCursor { selected: selected_line, offset: app.settings_cursor.offset };
    let range = lc.scroll_into_view(lines.len(), list.height as usize);
    app.settings_cursor.offset = lc.offset;
    let label_w = 18usize;
    for (row, li) in range.enumerate() {
        let r = line_at(list, row as u16);
        match lines[li] {
            (None, "") => {}
            (None, header) => Line::styled(header, theme.label()).render(r, buf),
            (Some(i), _) => {
                let selected = i == app.settings_cursor.selected;
                let base = row_style(&theme, selected, true);
                buf.set_style(r, base);
                let (label, value) = setting_text(app, rows[i]);
                let adjustable =
                    !matches!(rows[i], SettingRow::Folder(_) | SettingRow::AddFolder | SettingRow::Rescan);
                let value = if selected && adjustable { format!("‹ {value} ›") } else { value };
                let style = if matches!(rows[i], SettingRow::AddFolder | SettingRow::Rescan) {
                    base.patch(theme.accent())
                } else {
                    base
                };
                Line::from(vec![
                    Span::styled("  ", base),
                    Span::styled(fit(&label, label_w), base.patch(theme.muted())),
                    Span::styled(truncate(&value, (r.width as usize).saturating_sub(label_w + 2)), style),
                ])
                .render(r, buf);
            }
        }
    }
    Line::styled(
        truncate(&footer_note, parts[1].width as usize),
        if app.config_writable { theme.faint() } else { theme.warn() },
    )
    .render(parts[1], buf);
}
