//! Scrollable list cursor shared by every list view.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListCursor {
    pub selected: usize,
    /// First visible row; maintained by [`ListCursor::scroll_into_view`].
    pub offset: usize,
}

impl ListCursor {
    pub fn clamp(&mut self, len: usize) {
        if len == 0 {
            *self = Self::default();
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub fn move_by(&mut self, delta: isize, len: usize) {
        if len == 0 {
            *self = Self::default();
            return;
        }
        let max = len as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    pub fn top(&mut self) {
        self.selected = 0;
    }

    pub fn bottom(&mut self, len: usize) {
        self.selected = len.saturating_sub(1);
    }

    pub fn select(&mut self, idx: usize, len: usize) {
        self.selected = idx;
        self.clamp(len);
    }

    /// Adjusts `offset` so `selected` is visible in a window of `height` rows,
    /// keeping a small margin. Returns the visible range.
    pub fn scroll_into_view(&mut self, len: usize, height: usize) -> std::ops::Range<usize> {
        self.clamp(len);
        if height == 0 || len == 0 {
            self.offset = 0;
            return 0..0;
        }
        let margin = if height > 8 { 2 } else { 0 };
        if self.selected < self.offset + margin {
            self.offset = self.selected.saturating_sub(margin);
        } else if self.selected + margin >= self.offset + height {
            self.offset = (self.selected + margin + 1).saturating_sub(height);
        }
        self.offset = self.offset.min(len.saturating_sub(height));
        self.offset..(self.offset + height).min(len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_and_clamps() {
        let mut c = ListCursor::default();
        c.move_by(5, 3);
        assert_eq!(c.selected, 2);
        c.move_by(-10, 3);
        assert_eq!(c.selected, 0);
        c.move_by(1, 0);
        assert_eq!(c, ListCursor::default());
        c.selected = 9;
        c.clamp(4);
        assert_eq!(c.selected, 3);
    }

    #[test]
    fn scrolls_to_keep_selection_visible() {
        let mut c = ListCursor::default();
        assert_eq!(c.scroll_into_view(100, 10), 0..10);
        c.selected = 50;
        let r = c.scroll_into_view(100, 10);
        assert!(r.contains(&50) && r.len() == 10);
        c.bottom(100);
        assert_eq!(c.scroll_into_view(100, 10), 90..100);
        c.top();
        assert_eq!(c.scroll_into_view(100, 10), 0..10);
        assert_eq!(c.scroll_into_view(3, 10), 0..3);
        assert_eq!(c.scroll_into_view(5, 0), 0..0);
    }
}
