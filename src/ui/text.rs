//! Width-aware text helpers (CJK and emoji occupy two cells).

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncates to at most `max` cells, ending with `…` when cut.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.push('…');
    out
}

/// Truncates, then pads with spaces to exactly `w` cells.
pub fn fit(s: &str, w: usize) -> String {
    let mut t = truncate(s, w);
    let pad = w.saturating_sub(width(&t));
    t.extend(std::iter::repeat_n(' ', pad));
    t
}

/// Right-aligns within `w` cells.
pub fn fit_right(s: &str, w: usize) -> String {
    let t = truncate(s, w);
    let pad = w.saturating_sub(width(&t));
    let mut out: String = std::iter::repeat_n(' ', pad).collect();
    out.push_str(&t);
    out
}

/// `1234567` → `1,234,567`.
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", thousands(n), if n == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_display_width() {
        assert_eq!(truncate("Midnight City", 20), "Midnight City");
        assert_eq!(truncate("Midnight City", 9), "Midnight…");
        assert_eq!(truncate("Midnight City", 1), "…");
        assert_eq!(truncate("abc", 0), "");
        // Each CJK char is two cells wide.
        let t = truncate("東京事変のうた", 7);
        assert!(width(&t) <= 7, "{t}");
        assert_eq!(width(&fit("東京", 7)), 7);
    }

    #[test]
    fn alignment_and_numbers() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit_right("ab", 4), "  ab");
        assert_eq!(thousands(1204), "1,204");
        assert_eq!(thousands(12), "12");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(plural(1, "track", "tracks"), "1 track");
        assert_eq!(plural(3, "track", "tracks"), "3 tracks");
    }
}
