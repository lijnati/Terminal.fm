//! Semantic color tokens. Views never use raw colors — only these tokens.

use ratatui::style::{Color, Modifier, Style};

use crate::storage::ThemeName;

/// What the terminal can display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    Ansi256,
    /// The 16 standard colors.
    Basic,
}

impl ColorDepth {
    /// Best guess from the environment (`COLORTERM`, `TERM`, `NO_COLOR`).
    pub fn detect() -> Self {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default().to_ascii_lowercase();
        let term = std::env::var("TERM").unwrap_or_default().to_ascii_lowercase();
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return ColorDepth::Basic;
        }
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            ColorDepth::TrueColor
        } else if term.contains("256") || cfg!(windows) {
            ColorDepth::Ansi256
        } else {
            ColorDepth::Basic
        }
    }
}

/// One token: a precise RGB value plus a hand-picked 16-color fallback.
#[derive(Debug, Clone, Copy)]
struct Tok(u8, u8, u8, Color);

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: ThemeName,
    /// Painted behind everything (`Color::Reset` keeps the terminal's own background).
    pub bg: Color,
    pub fg: Color,
    /// Secondary text: artists, durations, labels.
    pub muted: Color,
    /// Rules, separators, inactive glyphs.
    pub faint: Color,
    pub accent: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub warn: Color,
    pub error: Color,
    /// Visualizer gradient from quiet to loud, plus peak markers.
    pub viz: [Color; 3],
    pub viz_peak: Color,
}

struct Palette {
    bg: Option<Tok>,
    fg: Tok,
    muted: Tok,
    faint: Tok,
    accent: Tok,
    sel_bg: Tok,
    sel_fg: Tok,
    warn: Tok,
    error: Tok,
    viz: [Tok; 3],
    peak: Tok,
}

fn palette(name: ThemeName) -> Palette {
    use Color as C;
    match name {
        ThemeName::Mono => Palette {
            bg: None,
            fg: Tok(0xe4, 0xe4, 0xe4, C::White),
            muted: Tok(0x8c, 0x8c, 0x8c, C::Gray),
            faint: Tok(0x44, 0x44, 0x44, C::DarkGray),
            accent: Tok(0xff, 0xff, 0xff, C::White),
            sel_bg: Tok(0x30, 0x30, 0x30, C::DarkGray),
            sel_fg: Tok(0xff, 0xff, 0xff, C::White),
            warn: Tok(0xd0, 0xd0, 0xd0, C::White),
            error: Tok(0xff, 0xff, 0xff, C::White),
            viz: [
                Tok(0x5a, 0x5a, 0x5a, C::DarkGray),
                Tok(0xa0, 0xa0, 0xa0, C::Gray),
                Tok(0xee, 0xee, 0xee, C::White),
            ],
            peak: Tok(0xff, 0xff, 0xff, C::White),
        },
        ThemeName::Midnight => Palette {
            bg: Some(Tok(0x0d, 0x10, 0x17, C::Reset)),
            fg: Tok(0xd5, 0xdb, 0xe6, C::White),
            muted: Tok(0x78, 0x82, 0x95, C::Gray),
            faint: Tok(0x26, 0x2d, 0x3a, C::DarkGray),
            accent: Tok(0x86, 0xa8, 0xf0, C::LightBlue),
            sel_bg: Tok(0x1a, 0x22, 0x33, C::Blue),
            sel_fg: Tok(0xf2, 0xf5, 0xfa, C::White),
            warn: Tok(0xe0, 0xb0, 0x6a, C::Yellow),
            error: Tok(0xf0, 0x7a, 0x8a, C::LightRed),
            viz: [
                Tok(0x2c, 0x48, 0x70, C::Blue),
                Tok(0x5c, 0x8a, 0xd8, C::LightBlue),
                Tok(0xa8, 0xcb, 0xff, C::Cyan),
            ],
            peak: Tok(0xe8, 0xf0, 0xff, C::White),
        },
        ThemeName::Amber => Palette {
            bg: Some(Tok(0x12, 0x0d, 0x06, C::Reset)),
            fg: Tok(0xf2, 0xb8, 0x5c, C::Yellow),
            muted: Tok(0x9a, 0x72, 0x3e, C::DarkGray),
            faint: Tok(0x3a, 0x2b, 0x16, C::DarkGray),
            accent: Tok(0xff, 0xb3, 0x1a, C::LightYellow),
            sel_bg: Tok(0x2e, 0x21, 0x0e, C::DarkGray),
            sel_fg: Tok(0xff, 0xd4, 0x80, C::LightYellow),
            warn: Tok(0xff, 0xc8, 0x57, C::LightYellow),
            error: Tok(0xff, 0x6b, 0x3d, C::LightRed),
            viz: [
                Tok(0x6e, 0x4a, 0x12, C::Yellow),
                Tok(0xc8, 0x8a, 0x1e, C::Yellow),
                Tok(0xff, 0xb8, 0x2e, C::LightYellow),
            ],
            peak: Tok(0xff, 0xe4, 0xaa, C::White),
        },
        ThemeName::Paper => Palette {
            bg: Some(Tok(0xf4, 0xf0, 0xe8, C::White)),
            fg: Tok(0x26, 0x24, 0x21, C::Black),
            muted: Tok(0x6e, 0x68, 0x5e, C::DarkGray),
            faint: Tok(0xd4, 0xcd, 0xbf, C::Gray),
            accent: Tok(0xb2, 0x43, 0x2b, C::Red),
            sel_bg: Tok(0xe4, 0xdc, 0xcb, C::Gray),
            sel_fg: Tok(0x14, 0x13, 0x11, C::Black),
            warn: Tok(0x9a, 0x6a, 0x10, C::Yellow),
            error: Tok(0xb2, 0x2b, 0x2b, C::Red),
            viz: [
                Tok(0xc4, 0xb5, 0x9e, C::Gray),
                Tok(0x8a, 0x6e, 0x56, C::DarkGray),
                Tok(0xb2, 0x43, 0x2b, C::Red),
            ],
            peak: Tok(0x26, 0x24, 0x21, C::Black),
        },
    }
}

/// Nearest xterm-256 index for an RGB color (6×6×6 cube or grayscale ramp).
pub fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    const STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let nearest =
        |v: u8| STEPS.iter().enumerate().min_by_key(|(_, s)| (**s as i32 - v as i32).abs()).unwrap().0;
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube = (STEPS[ri], STEPS[gi], STEPS[bi]);
    let dist = |(x, y, z): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(x, r) + d(y, g) + d(z, b)
    };
    let avg = (r as u32 + g as u32 + b as u32) / 3;
    let gi_ = ((avg.saturating_sub(8)) / 10).min(23) as u8;
    let gray = 8 + gi_ * 10;
    if dist((gray, gray, gray)) < dist(cube) {
        232 + gi_
    } else {
        16 + 36 * ri as u8 + 6 * gi as u8 + bi as u8
    }
}

impl Theme {
    pub fn new(name: ThemeName, depth: ColorDepth) -> Self {
        let p = palette(name);
        let c = |t: Tok| match depth {
            ColorDepth::TrueColor => Color::Rgb(t.0, t.1, t.2),
            ColorDepth::Ansi256 => Color::Indexed(rgb_to_256(t.0, t.1, t.2)),
            ColorDepth::Basic => t.3,
        };
        Theme {
            name,
            bg: p.bg.map(c).unwrap_or(Color::Reset),
            fg: c(p.fg),
            muted: c(p.muted),
            faint: c(p.faint),
            accent: c(p.accent),
            selection_bg: c(p.sel_bg),
            selection_fg: c(p.sel_fg),
            warn: c(p.warn),
            error: c(p.error),
            viz: p.viz.map(c),
            viz_peak: c(p.peak),
        }
    }

    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }
    pub fn text(&self) -> Style {
        Style::new().fg(self.fg)
    }
    pub fn muted(&self) -> Style {
        Style::new().fg(self.muted)
    }
    pub fn faint(&self) -> Style {
        Style::new().fg(self.faint)
    }
    pub fn accent(&self) -> Style {
        Style::new().fg(self.accent)
    }
    pub fn strong(&self) -> Style {
        Style::new().fg(self.fg).add_modifier(Modifier::BOLD)
    }
    /// Uppercase section labels.
    pub fn label(&self) -> Style {
        Style::new().fg(self.muted).add_modifier(Modifier::BOLD)
    }
    pub fn selected(&self, focused: bool) -> Style {
        if focused {
            Style::new().fg(self.selection_fg).bg(self.selection_bg).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.fg).bg(self.selection_bg)
        }
    }
    pub fn warn(&self) -> Style {
        Style::new().fg(self.warn)
    }
    pub fn error(&self) -> Style {
        Style::new().fg(self.error).add_modifier(Modifier::BOLD)
    }
    /// Visualizer color for a normalized height (0.0–1.0).
    pub fn viz_at(&self, level: f32) -> Color {
        match level {
            l if l < 0.4 => self.viz[0],
            l if l < 0.75 => self.viz[1],
            _ => self.viz[2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizes_to_xterm_256() {
        assert_eq!(rgb_to_256(0, 0, 0), 16);
        assert_eq!(rgb_to_256(255, 255, 255), 231);
        assert_eq!(rgb_to_256(255, 0, 0), 196);
        assert_eq!(rgb_to_256(0x80, 0x80, 0x80), 244);
    }

    #[test]
    fn every_theme_builds_at_every_depth() {
        for name in ThemeName::ALL {
            for depth in [ColorDepth::TrueColor, ColorDepth::Ansi256, ColorDepth::Basic] {
                let t = Theme::new(name, depth);
                assert_ne!(t.fg, t.bg, "{name:?} {depth:?}");
                assert_ne!(t.selection_fg, t.selection_bg, "{name:?} {depth:?}");
            }
        }
    }

    #[test]
    fn mono_keeps_terminal_background() {
        assert_eq!(Theme::new(ThemeName::Mono, ColorDepth::TrueColor).bg, Color::Reset);
        assert!(matches!(Theme::new(ThemeName::Paper, ColorDepth::TrueColor).bg, Color::Rgb(..)));
    }
}
