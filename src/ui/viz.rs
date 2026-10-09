//! Visualizer renderers. They only map analysis numbers to glyphs; all signal
//! processing happens in `crate::analysis`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use super::theme::Theme;
use crate::analysis::Frame;

const EIGHTHS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Bar layout for a given width: `(bar_count, bar_width, gap, left_pad)`.
pub fn spectrum_layout(width: u16, requested: u16) -> (usize, usize, usize, usize) {
    let w = width as usize;
    if w == 0 {
        return (0, 0, 0, 0);
    }
    let n = if requested > 0 { (requested as usize).min(w) } else { (w / 3).clamp(4.min(w), 64) };
    let gap = if w >= n * 2 { 1 } else { 0 };
    let bar_w = ((w + gap) / n).saturating_sub(gap).max(1);
    let n = n.min((w + gap) / (bar_w + gap)).max(1);
    let total = n * bar_w + (n - 1) * gap;
    (n, bar_w, gap, w.saturating_sub(total) / 2)
}

pub fn spectrum(buf: &mut Buffer, area: Rect, frame: &Frame, theme: &Theme, peaks: bool, requested: u16) {
    if area.width == 0 || area.height == 0 || frame.bands.is_empty() {
        return;
    }
    let (n, bar_w, gap, pad) = spectrum_layout(area.width, requested);
    let h = area.height as usize;
    for i in 0..n.min(frame.bands.len()) {
        let v = frame.bands[i].clamp(0.0, 1.0);
        let eighths = (v * (h * 8) as f32).round() as usize;
        let x0 = area.x as usize + pad + i * (bar_w + gap);
        for row in 0..h {
            // row 0 is the bottom line.
            let filled = eighths.saturating_sub(row * 8).min(8);
            let y = area.y as usize + h - 1 - row;
            let color = theme.viz_at((row as f32 + 0.5) / h as f32);
            let glyph = EIGHTHS[filled];
            if filled > 0 {
                for dx in 0..bar_w {
                    if let Some(cell) = buf.cell_mut((x0 as u16 + dx as u16, y as u16)) {
                        cell.set_char(glyph).set_fg(color);
                    }
                }
            }
        }
        if peaks {
            let p = frame.peaks.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            let prow = ((p * h as f32) as usize).min(h - 1);
            // Only when the peak floats clearly above its bar.
            if p > 0.02 && p * (h * 8) as f32 > eighths as f32 + 4.0 && prow * 8 >= eighths {
                let y = area.y as usize + h - 1 - prow;
                for dx in 0..bar_w {
                    if let Some(cell) = buf.cell_mut((x0 as u16 + dx as u16, y as u16)) {
                        cell.set_char('▔').set_fg(theme.viz_peak);
                    }
                }
            }
        }
    }
}

/// Plots an envelope of `(min, max)` points (−1.0–1.0, two per cell horizontally)
/// as a continuous braille trace.
pub fn waveform(buf: &mut Buffer, area: Rect, points: &[(f32, f32)], theme: &Theme) {
    let (w, h) = (area.width as usize, area.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let dots_y = h * 4;
    let mut cells = vec![0u8; w * h];
    // Braille dot bits by (column, row within cell).
    const BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
    let to_row = |v: f32| -> usize {
        let v = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
        (((1.0 - v) * 0.5) * (dots_y - 1) as f32).round() as usize
    };
    let mut prev: Option<(usize, usize)> = None;
    for (x, &(vlo, vhi)) in points.iter().enumerate().take(w * 2) {
        // Screen rows grow downward: the max value is the top row.
        let (mut top, mut bottom) = (to_row(vhi), to_row(vlo));
        if top > bottom {
            std::mem::swap(&mut top, &mut bottom);
        }
        // Bridge gaps to the previous column so the trace stays continuous.
        if let Some((pt, pb)) = prev {
            if pb < top {
                top = pb;
            } else if pt > bottom {
                bottom = pt;
            }
        }
        for row in top..=bottom {
            let (cx, cy) = (x / 2, row / 4);
            cells[cy * w + cx] |= BITS[x % 2][row % 4];
        }
        prev = Some((to_row(vhi).min(to_row(vlo)), to_row(vhi).max(to_row(vlo))));
    }
    let mid = h / 2;
    for cy in 0..h {
        for cx in 0..w {
            let bits = cells[cy * w + cx];
            let pos = (area.x + cx as u16, area.y + cy as u16);
            if bits != 0 {
                let ch = char::from_u32(0x2800 + bits as u32).unwrap_or(' ');
                let dist = (cy as f32 + 0.5 - h as f32 / 2.0).abs() / (h as f32 / 2.0);
                buf[pos].set_char(ch).set_fg(theme.viz_at(0.35 + dist * 0.65));
            } else if cy == mid && h > 2 && cx % 2 == 0 {
                buf[pos].set_char('·').set_fg(theme.faint);
            }
        }
    }
}

/// A restrained, centered level line that breathes with the music.
pub fn minimal(buf: &mut Buffer, area: Rect, frame: &Frame, theme: &Theme) {
    if area.width < 3 || area.height == 0 {
        return;
    }
    let y = area.y + area.height / 2;
    let half = (area.width as usize - 1) / 2;
    let cx = area.x as usize + half;
    let core = (frame.rms.clamp(0.0, 1.0) * half as f32).round() as usize;
    let edge = ((frame.peak.clamp(0.0, 1.0) * half as f32).round() as usize).max(core);
    // Faint full-width guide so the quiet state still reads as "listening".
    for x in 0..area.width {
        buf[(area.x + x, y)].set_char('─').set_style(Style::new().fg(theme.faint));
    }
    for d in 0..=edge {
        let (ch, color) = if d <= core {
            ('━', theme.viz_at(1.0 - d as f32 / half.max(1) as f32 * 0.6))
        } else {
            ('─', theme.viz[0])
        };
        for x in [cx.saturating_sub(d), cx + d] {
            if x >= area.x as usize && x < (area.x + area.width) as usize {
                buf[(x as u16, y)].set_char(ch).set_fg(color);
            }
        }
    }
    buf[(cx as u16, y)].set_char('●').set_fg(if core > 0 { theme.viz_peak } else { theme.faint });
}

/// One-line level meter (used in the mini player).
pub fn level_meter(level: f32, cells: usize) -> String {
    let filled = (level.clamp(0.0, 1.0) * (cells * 8) as f32).round() as usize;
    (0..cells).map(|i| EIGHTHS[filled.saturating_sub(i * 8).min(8)]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::ThemeName;
    use crate::ui::theme::ColorDepth;

    fn theme() -> Theme {
        Theme::new(ThemeName::Mono, ColorDepth::TrueColor)
    }

    #[test]
    fn layout_fits_inside_width() {
        for w in [1u16, 2, 5, 10, 40, 80, 200, 400] {
            for req in [0u16, 3, 16, 300] {
                let (n, bw, gap, pad) = spectrum_layout(w, req);
                assert!(n >= 1 && bw >= 1, "w={w} req={req}");
                assert!(pad + n * bw + (n - 1) * gap <= w as usize, "w={w} req={req}");
            }
        }
        assert_eq!(spectrum_layout(0, 0).0, 0);
    }

    #[test]
    fn full_bar_reaches_top_and_empty_bar_draws_nothing() {
        let area = Rect::new(0, 0, 9, 4);
        let mut buf = Buffer::empty(area);
        let f = Frame { bands: vec![1.0, 0.0, 0.5], peaks: vec![0.0; 3], rms: 0.0, peak: 0.0 };
        spectrum(&mut buf, area, &f, &theme(), true, 3);
        let col = |x: u16| (0..4).map(|y| buf[(x, y)].symbol().to_string()).collect::<Vec<_>>().join("");
        let (_, bw, gap, pad) = spectrum_layout(9, 3);
        assert_eq!(col(pad as u16), "████");
        assert_eq!(col((pad + bw + gap) as u16), "    ");
        assert_eq!(col((pad + 2 * (bw + gap)) as u16), "  ██");
    }

    #[test]
    fn renderers_survive_tiny_and_degenerate_areas() {
        let f = Frame { bands: vec![0.7; 64], peaks: vec![0.9; 64], rms: 0.5, peak: 0.8 };
        for (w, h) in [(0, 0), (1, 1), (2, 1), (3, 1), (1, 5)] {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            spectrum(&mut buf, area, &f, &theme(), true, 0);
            waveform(&mut buf, area, &[(0.5, 0.5), (-0.5, 0.2), (f32::NAN, f32::NAN)], &theme());
            minimal(&mut buf, area, &f, &theme());
        }
    }

    #[test]
    fn waveform_draws_a_connected_trace() {
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        let pts: Vec<(f32, f32)> =
            (0..20).map(|i| if i % 2 == 0 { (0.9, 0.9) } else { (-0.9, -0.9) }).collect();
        waveform(&mut buf, area, &pts, &theme());
        let braille = (0..3)
            .flat_map(|y| (0..10).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                buf[(x, y)].symbol().chars().next().is_some_and(|c| ('\u{2801}'..='\u{28FF}').contains(&c))
            })
            .count();
        assert!(braille >= 20, "trace should span top to bottom in every column: {braille}");
    }

    #[test]
    fn level_meter_scales() {
        assert_eq!(level_meter(0.0, 4), "    ");
        assert_eq!(level_meter(1.0, 4), "████");
        assert_eq!(level_meter(0.5, 2), "█ ");
    }
}
