//! Playback queue, shuffle and repeat logic. Pure: no audio or UI dependencies.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn cycle(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "OFF",
            Repeat::All => "ALL",
            Repeat::One => "ONE",
        }
    }
}

/// Tiny deterministic PRNG (xorshift64*) so shuffle is testable without extra deps.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn from_time() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self::new(nanos ^ 0x9E37_79B9_7F4A_7C15)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform-ish value in `0..n` (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i + 1);
            v.swap(i, j);
        }
    }
}

/// Why the queue is advancing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Advance {
    /// The current track finished on its own: honours `Repeat::One`.
    Auto,
    /// The user pressed next: always moves on.
    User,
}

/// An ordered list of items `T` with a cursor, shuffle and repeat.
///
/// `items` keeps insertion order (what the user sees); `order` is the play
/// order — identity when shuffle is off, a permutation when on.
#[derive(Debug, Clone)]
pub struct Queue<T> {
    items: Vec<T>,
    order: Vec<usize>,
    /// Position within `order`.
    pos: Option<usize>,
    shuffle: bool,
    repeat: Repeat,
    rng: Rng,
}

impl<T: Clone + PartialEq> Queue<T> {
    pub fn new(rng: Rng) -> Self {
        Self { items: Vec::new(), order: Vec::new(), pos: None, shuffle: false, repeat: Repeat::Off, rng }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }
    pub fn repeat(&self) -> Repeat {
        self.repeat
    }
    pub fn set_repeat(&mut self, r: Repeat) {
        self.repeat = r;
    }

    /// Position of the current entry in play order.
    pub fn position(&self) -> Option<usize> {
        self.pos
    }

    /// Index into `items` of the current entry.
    pub fn current_index(&self) -> Option<usize> {
        self.pos.map(|p| self.order[p])
    }
    pub fn current(&self) -> Option<&T> {
        self.current_index().map(|i| &self.items[i])
    }

    fn rebuild_order(&mut self) {
        let cur = self.current_index();
        self.order = (0..self.items.len()).collect();
        if self.shuffle {
            self.rng.shuffle(&mut self.order);
            // Keep the playing item first so "next" walks the rest of the shuffle.
            if let Some(c) = cur {
                let at = self.order.iter().position(|&i| i == c).unwrap_or(0);
                self.order.swap(0, at);
            }
        }
        self.pos = cur.map(|c| self.order.iter().position(|&i| i == c).unwrap_or(0));
    }

    /// Replaces the queue and starts at `start` (index into `items`).
    pub fn replace(&mut self, items: Vec<T>, start: usize) -> Option<&T> {
        self.items = items;
        self.pos = None;
        if self.items.is_empty() {
            self.order.clear();
            return None;
        }
        let start = start.min(self.items.len() - 1);
        self.order = (0..self.items.len()).collect();
        self.pos = Some(start);
        self.rebuild_order();
        self.current()
    }

    pub fn push(&mut self, item: T) {
        self.items.push(item);
        let new = self.items.len() - 1;
        if self.shuffle {
            // Insert at a random position after the current one.
            let from = self.pos.map(|p| p + 1).unwrap_or(0);
            let at = from + self.rng.below(self.order.len() - from + 1);
            self.order.insert(at, new);
        } else {
            self.order.push(new);
        }
    }

    /// Inserts right after the current item in play order.
    pub fn push_next(&mut self, item: T) {
        self.items.push(item);
        let new = self.items.len() - 1;
        let at = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order.insert(at, new);
    }

    /// Removes the item at `items[idx]`. Returns true if it was the current item.
    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= self.items.len() {
            return false;
        }
        let was_current = self.current_index() == Some(idx);
        let opos = self.order.iter().position(|&i| i == idx).expect("order is a permutation");
        self.items.remove(idx);
        self.order.remove(opos);
        for i in &mut self.order {
            if *i > idx {
                *i -= 1;
            }
        }
        self.pos = match self.pos {
            _ if self.items.is_empty() => None,
            Some(p) if p > opos => Some(p - 1),
            Some(p) if p == opos => Some(p.min(self.order.len() - 1)),
            other => other,
        };
        was_current
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.pos = None;
    }

    /// Jumps to `items[idx]`.
    pub fn jump(&mut self, idx: usize) -> Option<&T> {
        let p = self.order.iter().position(|&i| i == idx)?;
        self.pos = Some(p);
        self.current()
    }

    pub fn toggle_shuffle(&mut self) -> bool {
        self.shuffle = !self.shuffle;
        self.rebuild_order();
        self.shuffle
    }

    pub fn cycle_repeat(&mut self) -> Repeat {
        self.repeat = self.repeat.cycle();
        self.repeat
    }

    /// Moves forward. Returns the new current item, or `None` when playback should stop.
    pub fn next(&mut self, why: Advance) -> Option<&T> {
        let p = self.pos?;
        if why == Advance::Auto && self.repeat == Repeat::One {
            return self.current();
        }
        if p + 1 < self.order.len() {
            self.pos = Some(p + 1);
        } else if self.repeat != Repeat::Off {
            if self.shuffle {
                // New shuffle each lap, avoiding an immediate repeat of the last item.
                let last = self.order[p];
                self.rng.shuffle(&mut self.order);
                if self.order.len() > 1 && self.order[0] == last {
                    let n = self.order.len();
                    self.order.swap(0, n - 1);
                }
            }
            self.pos = Some(0);
        } else {
            if why == Advance::Auto {
                self.pos = None;
            }
            return None;
        }
        self.current()
    }

    /// Moves backward (wrapping only when repeating).
    pub fn prev(&mut self) -> Option<&T> {
        let p = self.pos?;
        if p > 0 {
            self.pos = Some(p - 1);
        } else if self.repeat != Repeat::Off {
            self.pos = Some(self.order.len() - 1);
        }
        self.current()
    }

    /// The next `n` items in play order after the current one.
    pub fn upcoming(&self, n: usize) -> Vec<(usize, &T)> {
        let start = self.pos.map(|p| p + 1).unwrap_or(0);
        self.order.iter().skip(start).take(n).map(|&i| (i, &self.items[i])).collect()
    }

    /// Replaces items via `f`, dropping those mapped to `None` (used after rescans).
    pub fn remap(&mut self, mut f: impl FnMut(&T) -> Option<T>) {
        let cur = self.current_index();
        let mut new_items = Vec::new();
        let mut new_cur = None;
        let mut map = vec![None; self.items.len()];
        for (i, it) in self.items.iter().enumerate() {
            if let Some(n) = f(it) {
                map[i] = Some(new_items.len());
                if Some(i) == cur {
                    new_cur = Some(new_items.len());
                }
                new_items.push(n);
            }
        }
        let order: Vec<usize> = self.order.iter().filter_map(|&i| map[i]).collect();
        self.items = new_items;
        self.pos = new_cur.and_then(|c| order.iter().position(|&i| i == c));
        self.order = order;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: usize) -> Queue<usize> {
        let mut q = Queue::new(Rng::new(42));
        q.replace((0..n).collect(), 0);
        q
    }

    #[test]
    fn linear_progression_stops_at_end() {
        let mut q = q(3);
        assert_eq!(q.current(), Some(&0));
        assert_eq!(q.next(Advance::Auto), Some(&1));
        assert_eq!(q.next(Advance::Auto), Some(&2));
        assert_eq!(q.next(Advance::Auto), None);
        assert_eq!(q.current(), None);
    }

    #[test]
    fn user_next_at_end_keeps_current() {
        let mut q = q(2);
        q.next(Advance::User);
        assert_eq!(q.next(Advance::User), None);
        assert_eq!(q.current(), Some(&1));
    }

    #[test]
    fn repeat_all_wraps_both_ways() {
        let mut q = q(3);
        q.set_repeat(Repeat::All);
        assert_eq!(q.prev(), Some(&2));
        assert_eq!(q.next(Advance::Auto), Some(&0));
    }

    #[test]
    fn repeat_one_only_affects_auto_advance() {
        let mut q = q(3);
        q.set_repeat(Repeat::One);
        assert_eq!(q.next(Advance::Auto), Some(&0));
        assert_eq!(q.next(Advance::User), Some(&1));
    }

    #[test]
    fn repeat_cycles() {
        assert_eq!(Repeat::Off.cycle(), Repeat::All);
        assert_eq!(Repeat::All.cycle(), Repeat::One);
        assert_eq!(Repeat::One.cycle(), Repeat::Off);
    }

    #[test]
    fn shuffle_is_a_permutation_starting_with_current() {
        let mut q = q(50);
        q.jump(17);
        q.toggle_shuffle();
        assert_eq!(q.current(), Some(&17));
        let mut seen = vec![17];
        while let Some(&x) = q.next(Advance::Auto) {
            seen.push(x);
        }
        let mut sorted = seen.clone();
        sorted.sort();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(seen, (0..50).collect::<Vec<_>>(), "order should be shuffled");
    }

    #[test]
    fn shuffle_is_deterministic_for_a_seed() {
        let run = || {
            let mut q = q(20);
            q.toggle_shuffle();
            q.upcoming(19).into_iter().map(|(_, &x)| x).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn unshuffle_restores_insertion_order_and_keeps_current() {
        let mut q = q(10);
        q.toggle_shuffle();
        q.next(Advance::User);
        let cur = *q.current().unwrap();
        q.toggle_shuffle();
        assert_eq!(q.current(), Some(&cur));
        let up: Vec<_> = q.upcoming(100).into_iter().map(|(_, &x)| x).collect();
        assert_eq!(up, ((cur + 1)..10).collect::<Vec<_>>());
    }

    #[test]
    fn shuffle_repeat_all_reshuffles_without_immediate_repeat() {
        let mut q = q(5);
        q.set_repeat(Repeat::All);
        q.toggle_shuffle();
        for _ in 0..100 {
            let before = *q.current().unwrap();
            let after = *q.next(Advance::Auto).unwrap();
            assert_ne!(before, after);
        }
    }

    #[test]
    fn remove_adjusts_cursor() {
        let mut q = q(4);
        q.jump(2);
        assert!(!q.remove(0));
        assert_eq!(q.current(), Some(&2));
        assert!(q.remove(1)); // removing current (value 2, now at index 1)
        assert_eq!(q.current(), Some(&3));
        q.remove(1);
        assert_eq!(q.current(), Some(&1));
        q.remove(0);
        assert_eq!(q.current(), None);
        assert!(q.is_empty());
    }

    #[test]
    fn push_next_plays_after_current() {
        let mut q = q(3);
        q.push_next(99);
        assert_eq!(q.next(Advance::User), Some(&99));
        assert_eq!(q.next(Advance::User), Some(&1));
    }

    #[test]
    fn push_appends_and_starts_empty_queue_at_first() {
        let mut q: Queue<u8> = Queue::new(Rng::new(1));
        q.push(5);
        assert_eq!(q.current(), None);
        assert_eq!(q.upcoming(5).len(), 1);
    }

    #[test]
    fn remap_drops_missing_and_keeps_current() {
        let mut q = q(5);
        q.jump(3);
        q.remap(|&x| if x % 2 == 0 { None } else { Some(x * 10) });
        assert_eq!(q.items(), &[10, 30]);
        assert_eq!(q.current(), Some(&30));
    }

    #[test]
    fn empty_queue_is_safe() {
        let mut q: Queue<u8> = Queue::new(Rng::new(1));
        assert_eq!(q.next(Advance::Auto), None);
        assert_eq!(q.prev(), None);
        assert!(q.replace(vec![], 3).is_none());
        q.toggle_shuffle();
        assert!(!q.remove(0));
    }
}
