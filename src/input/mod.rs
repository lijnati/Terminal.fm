//! Keyboard bindings. The keymap table is the single source of truth for both
//! dispatch and the help overlay.

use std::collections::BTreeMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // Playback
    PlayPause,
    Stop,
    Next,
    Previous,
    SeekBack,
    SeekForward,
    VolumeUp,
    VolumeDown,
    Mute,
    ToggleShuffle,
    CycleRepeat,
    // Navigation
    Up,
    Down,
    Top,
    Bottom,
    PageUp,
    PageDown,
    Select,
    Back,
    NextPanel,
    PrevPanel,
    Screen(u8),
    // Library & playlists
    ToggleFavorite,
    Enqueue,
    PlayNext,
    AddToPlaylist,
    Create,
    Rename,
    Delete,
    MoveUp,
    MoveDown,
    Sort,
    JumpToCurrent,
    // App
    Search,
    CycleVisualizer,
    CycleTheme,
    Help,
    Quit,
}

impl Action {
    /// Stable name used for config remaps (`[keys]` table).
    pub fn name(self) -> String {
        match self {
            Action::Screen(n) => format!("screen_{n}"),
            other => {
                let dbg = format!("{other:?}");
                let mut out = String::new();
                for (i, c) in dbg.chars().enumerate() {
                    if c.is_uppercase() && i > 0 {
                        out.push('_');
                    }
                    out.extend(c.to_lowercase());
                }
                out
            }
        }
    }
}

/// A key without kind/state noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
}

impl Key {
    pub const fn plain(code: KeyCode) -> Self {
        Self { code, ctrl: false }
    }
    pub const fn ch(c: char) -> Self {
        Self::plain(KeyCode::Char(c))
    }
    pub const fn ctrl(c: char) -> Self {
        Self { code: KeyCode::Char(c), ctrl: true }
    }

    pub fn from_event(ev: &KeyEvent) -> Self {
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        // Shift is already reflected in the char ('G', '?', '+').
        let code = match ev.code {
            KeyCode::Char(c) if ctrl => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        Self { code, ctrl }
    }

    /// Parses config syntax: `"j"`, `"space"`, `"ctrl+d"`, `"f5"`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (ctrl, rest) = match s.to_ascii_lowercase().strip_prefix("ctrl+") {
            Some(_) => (true, &s[5..]),
            None => (false, s),
        };
        let mut chars = rest.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(c), None) => KeyCode::Char(if ctrl { c.to_ascii_lowercase() } else { c }),
            _ => match rest.to_ascii_lowercase().as_str() {
                "space" => KeyCode::Char(' '),
                "enter" | "return" => KeyCode::Enter,
                "tab" => KeyCode::Tab,
                "backtab" | "shift+tab" => KeyCode::BackTab,
                "esc" | "escape" => KeyCode::Esc,
                "up" => KeyCode::Up,
                "down" => KeyCode::Down,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                "home" => KeyCode::Home,
                "end" => KeyCode::End,
                "pgup" | "pageup" => KeyCode::PageUp,
                "pgdn" | "pagedown" => KeyCode::PageDown,
                "backspace" => KeyCode::Backspace,
                "delete" | "del" => KeyCode::Delete,
                f if f.starts_with('f') => KeyCode::F(f[1..].parse().ok().filter(|n| (1..=12).contains(n))?),
                _ => return None,
            },
        };
        Some(Self { code, ctrl })
    }

    pub fn label(&self) -> String {
        let base = match self.code {
            KeyCode::Char(' ') => "SPACE".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "ENTER".into(),
            KeyCode::Tab => "TAB".into(),
            KeyCode::BackTab => "S-TAB".into(),
            KeyCode::Esc => "ESC".into(),
            KeyCode::Up => "↑".into(),
            KeyCode::Down => "↓".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::Home => "HOME".into(),
            KeyCode::End => "END".into(),
            KeyCode::PageUp => "PGUP".into(),
            KeyCode::PageDown => "PGDN".into(),
            KeyCode::Backspace => "BKSP".into(),
            KeyCode::Delete => "DEL".into(),
            KeyCode::F(n) => format!("F{n}"),
            _ => "?".into(),
        };
        if self.ctrl { format!("^{}", base.to_uppercase()) } else { base }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Playback,
    Navigation,
    Library,
    App,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Playback => "PLAYBACK",
            Group::Navigation => "NAVIGATION",
            Group::Library => "LIBRARY & PLAYLISTS",
            Group::App => "APP",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub action: Action,
    pub keys: Vec<Key>,
    pub help: &'static str,
    pub group: Group,
}

fn defaults() -> Vec<Binding> {
    use Action::*;
    use Group as G;
    use KeyCode as K;
    let b = |action, keys: &[Key], help, group| Binding { action, keys: keys.to_vec(), help, group };
    vec![
        b(PlayPause, &[Key::ch(' ')], "play / pause", G::Playback),
        b(Next, &[Key::ch('n')], "next track", G::Playback),
        b(Previous, &[Key::ch('p')], "previous track (restart if > 3 s)", G::Playback),
        b(SeekBack, &[Key::ch('h'), Key::plain(K::Left)], "seek backward", G::Playback),
        b(SeekForward, &[Key::ch('l'), Key::plain(K::Right)], "seek forward", G::Playback),
        b(VolumeUp, &[Key::ch('+'), Key::ch('=')], "volume up", G::Playback),
        b(VolumeDown, &[Key::ch('-'), Key::ch('_')], "volume down", G::Playback),
        b(Mute, &[Key::ch('m')], "mute", G::Playback),
        b(ToggleShuffle, &[Key::ch('s')], "toggle shuffle", G::Playback),
        b(CycleRepeat, &[Key::ch('r')], "repeat off → all → one", G::Playback),
        b(Stop, &[Key::ch('x')], "stop", G::Playback),
        b(Down, &[Key::ch('j'), Key::plain(K::Down)], "move down", G::Navigation),
        b(Up, &[Key::ch('k'), Key::plain(K::Up)], "move up", G::Navigation),
        b(Top, &[Key::ch('g'), Key::plain(K::Home)], "jump to top", G::Navigation),
        b(Bottom, &[Key::ch('G'), Key::plain(K::End)], "jump to bottom", G::Navigation),
        b(PageDown, &[Key::plain(K::PageDown), Key::ctrl('d')], "page down", G::Navigation),
        b(PageUp, &[Key::plain(K::PageUp), Key::ctrl('u')], "page up", G::Navigation),
        b(Select, &[Key::plain(K::Enter)], "play / open selection", G::Navigation),
        b(Back, &[Key::plain(K::Esc), Key::plain(K::Backspace)], "back / close", G::Navigation),
        b(NextPanel, &[Key::plain(K::Tab)], "next panel / tab", G::Navigation),
        b(PrevPanel, &[Key::plain(K::BackTab)], "previous panel / tab", G::Navigation),
        b(Screen(1), &[Key::ch('1')], "now playing", G::Navigation),
        b(Screen(2), &[Key::ch('2')], "library", G::Navigation),
        b(Screen(3), &[Key::ch('3')], "playlists", G::Navigation),
        b(Screen(4), &[Key::ch('4')], "favorites", G::Navigation),
        b(Screen(5), &[Key::ch('5')], "recently played", G::Navigation),
        b(Screen(6), &[Key::ch('6')], "settings", G::Navigation),
        b(JumpToCurrent, &[Key::ch('.')], "select the playing track", G::Navigation),
        b(ToggleFavorite, &[Key::ch('f')], "toggle favorite", G::Library),
        b(Enqueue, &[Key::ch('a')], "add to queue", G::Library),
        b(PlayNext, &[Key::ch('e')], "play next (insert after current)", G::Library),
        b(AddToPlaylist, &[Key::ch('A')], "add to playlist…", G::Library),
        b(Create, &[Key::ch('c')], "new playlist / add folder", G::Library),
        b(Rename, &[Key::ch('R')], "rename playlist", G::Library),
        b(Delete, &[Key::ch('d'), Key::plain(K::Delete)], "remove / delete", G::Library),
        b(MoveDown, &[Key::ch('J')], "move item down", G::Library),
        b(MoveUp, &[Key::ch('K')], "move item up", G::Library),
        b(Sort, &[Key::ch('o')], "change sort order", G::Library),
        b(Search, &[Key::ch('/')], "search", G::App),
        b(CycleVisualizer, &[Key::ch('v')], "change visualizer", G::App),
        b(CycleTheme, &[Key::ch('t')], "change theme", G::App),
        b(Help, &[Key::ch('?'), Key::plain(K::F(1))], "help", G::App),
        b(Quit, &[Key::ch('q'), Key::ctrl('c')], "quit", G::App),
    ]
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self { bindings: defaults() }
    }
}

impl Keymap {
    /// Applies `[keys]` remaps. A remap replaces the action's primary key and
    /// unbinds that key from any other action. Returns human-readable problems.
    pub fn with_overrides(overrides: &BTreeMap<String, String>) -> (Self, Vec<String>) {
        let mut map = Self::default();
        let mut problems = Vec::new();
        for (name, key) in overrides {
            let Some(idx) = map.bindings.iter().position(|b| b.action.name() == *name) else {
                problems.push(format!("unknown action '{name}'"));
                continue;
            };
            let Some(k) = Key::parse(key) else {
                problems.push(format!("unrecognized key '{key}' for '{name}'"));
                continue;
            };
            if map.bindings[idx].action == Action::Quit && k == Key::ctrl('c') {
                continue;
            }
            for b in &mut map.bindings {
                b.keys.retain(|x| *x != k);
            }
            let keys = &mut map.bindings[idx].keys;
            if keys.is_empty() {
                keys.push(k);
            } else {
                keys[0] = k;
            }
        }
        // Ctrl+C always quits so the user can never get stuck.
        if let Some(q) = map.bindings.iter_mut().find(|b| b.action == Action::Quit) {
            if !q.keys.contains(&Key::ctrl('c')) {
                q.keys.push(Key::ctrl('c'));
            }
        }
        (map, problems)
    }

    pub fn lookup(&self, key: Key) -> Option<Action> {
        self.bindings.iter().find(|b| b.keys.contains(&key)).map(|b| b.action)
    }

    pub fn resolve(&self, ev: &KeyEvent) -> Option<Action> {
        self.lookup(Key::from_event(ev))
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    /// Primary key label for an action (for hints), e.g. `"SPACE"`.
    pub fn label(&self, action: Action) -> String {
        self.bindings
            .iter()
            .find(|b| b.action == action)
            .and_then(|b| b.keys.first())
            .map(Key::label)
            .unwrap_or_else(|| "—".into())
    }
}

/// Result of feeding a key to a single-line text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOutcome {
    Edited,
    Submit,
    Cancel,
    /// The key isn't a text-editing key (arrows, tab…); caller may handle it.
    Ignored,
}

/// A minimal single-line editor with a cursor (char index).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.value.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(self.value.len())
    }

    pub fn set(&mut self, value: impl Into<String>) {
        *self = Self::new(value);
    }

    /// Handles a key. Ctrl+U clears, Ctrl+W deletes a word.
    pub fn handle(&mut self, ev: &KeyEvent) -> TextOutcome {
        let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        match ev.code {
            KeyCode::Enter => TextOutcome::Submit,
            KeyCode::Esc => TextOutcome::Cancel,
            KeyCode::Char('u') if ctrl => {
                self.set("");
                TextOutcome::Edited
            }
            KeyCode::Char('w') if ctrl => {
                let before: String = self.value.chars().take(self.cursor).collect();
                let trimmed = before.trim_end();
                let cut = trimmed.rfind([' ', '/', '\\']).map(|i| i + 1).unwrap_or(0);
                let keep: String = trimmed[..cut].to_string();
                let after: String = self.value.chars().skip(self.cursor).collect();
                self.cursor = keep.chars().count();
                self.value = keep + &after;
                TextOutcome::Edited
            }
            KeyCode::Char(_) if ctrl => TextOutcome::Ignored,
            KeyCode::Char(c) => {
                let at = self.byte_at(self.cursor);
                self.value.insert(at, c);
                self.cursor += 1;
                TextOutcome::Edited
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    let at = self.byte_at(self.cursor);
                    self.value.remove(at);
                }
                TextOutcome::Edited
            }
            KeyCode::Delete => {
                if self.cursor < self.value.chars().count() {
                    let at = self.byte_at(self.cursor);
                    self.value.remove(at);
                }
                TextOutcome::Edited
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                TextOutcome::Edited
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.value.chars().count());
                TextOutcome::Edited
            }
            KeyCode::Home => {
                self.cursor = 0;
                TextOutcome::Edited
            }
            KeyCode::End => {
                self.cursor = self.value.chars().count();
                TextOutcome::Edited
            }
            _ => TextOutcome::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEventKind, KeyEventState};

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers: mods, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }
    fn key(c: char) -> KeyEvent {
        ev(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn default_bindings_match_the_spec() {
        let k = Keymap::default();
        assert_eq!(k.resolve(&key(' ')), Some(Action::PlayPause));
        assert_eq!(k.resolve(&key('j')), Some(Action::Down));
        assert_eq!(k.resolve(&key('k')), Some(Action::Up));
        assert_eq!(k.resolve(&ev(KeyCode::Enter, KeyModifiers::NONE)), Some(Action::Select));
        assert_eq!(k.resolve(&key('n')), Some(Action::Next));
        assert_eq!(k.resolve(&key('p')), Some(Action::Previous));
        assert_eq!(k.resolve(&key('h')), Some(Action::SeekBack));
        assert_eq!(k.resolve(&key('l')), Some(Action::SeekForward));
        assert_eq!(k.resolve(&key('+')), Some(Action::VolumeUp));
        assert_eq!(k.resolve(&key('-')), Some(Action::VolumeDown));
        assert_eq!(k.resolve(&key('/')), Some(Action::Search));
        assert_eq!(k.resolve(&key('v')), Some(Action::CycleVisualizer));
        assert_eq!(k.resolve(&key('s')), Some(Action::ToggleShuffle));
        assert_eq!(k.resolve(&key('r')), Some(Action::CycleRepeat));
        assert_eq!(k.resolve(&key('f')), Some(Action::ToggleFavorite));
        assert_eq!(k.resolve(&ev(KeyCode::Tab, KeyModifiers::NONE)), Some(Action::NextPanel));
        assert_eq!(k.resolve(&key('?')), Some(Action::Help));
        assert_eq!(k.resolve(&key('q')), Some(Action::Quit));
        assert_eq!(k.resolve(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(Action::Quit));
        // Shifted letters are distinct actions.
        assert_eq!(k.resolve(&ev(KeyCode::Char('G'), KeyModifiers::SHIFT)), Some(Action::Bottom));
    }

    #[test]
    fn no_key_is_bound_twice() {
        let k = Keymap::default();
        let mut seen = std::collections::HashSet::new();
        for b in k.bindings() {
            for key in &b.keys {
                assert!(seen.insert(*key), "{key:?} bound twice");
            }
        }
    }

    #[test]
    fn overrides_rebind_and_steal_keys() {
        let mut o = BTreeMap::new();
        o.insert("play_pause".to_string(), "p".to_string());
        o.insert("bogus".to_string(), "z".to_string());
        o.insert("next".to_string(), "ctrl+?".to_string());
        o.insert("quit".to_string(), "Q".to_string());
        let (k, problems) = Keymap::with_overrides(&o);
        assert_eq!(k.resolve(&key('p')), Some(Action::PlayPause));
        assert_eq!(k.resolve(&key(' ')), None);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(k.resolve(&key('Q')), Some(Action::Quit));
        assert_eq!(k.resolve(&ev(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(Action::Quit));
    }

    #[test]
    fn key_parsing() {
        assert_eq!(Key::parse("space"), Some(Key::ch(' ')));
        assert_eq!(Key::parse("ctrl+D"), Some(Key::ctrl('d')));
        assert_eq!(Key::parse("F5"), Some(Key::plain(KeyCode::F(5))));
        assert_eq!(Key::parse("x"), Some(Key::ch('x')));
        assert_eq!(Key::parse("f13"), None);
        assert_eq!(Key::parse("hyper"), None);
    }

    #[test]
    fn action_names_are_snake_case() {
        assert_eq!(Action::PlayPause.name(), "play_pause");
        assert_eq!(Action::Screen(3).name(), "screen_3");
        assert_eq!(Action::CycleVisualizer.name(), "cycle_visualizer");
    }

    #[test]
    fn text_input_editing() {
        let mut t = TextInput::new("helo");
        assert_eq!(t.handle(&ev(KeyCode::Left, KeyModifiers::NONE)), TextOutcome::Edited);
        t.handle(&key('l'));
        assert_eq!(t.value, "hello");
        t.handle(&ev(KeyCode::End, KeyModifiers::NONE));
        t.handle(&key(' '));
        t.handle(&key('w'));
        assert_eq!(t.value, "hello w");
        t.handle(&ev(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(t.value, "hello ");
        t.handle(&ev(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(t.value, "hello");
        // Playback shortcuts are plain text here.
        assert_eq!(t.handle(&key('q')), TextOutcome::Edited);
        assert_eq!(t.value, "helloq");
        assert_eq!(t.handle(&ev(KeyCode::Enter, KeyModifiers::NONE)), TextOutcome::Submit);
        assert_eq!(t.handle(&ev(KeyCode::Esc, KeyModifiers::NONE)), TextOutcome::Cancel);
        assert_eq!(t.handle(&ev(KeyCode::Down, KeyModifiers::NONE)), TextOutcome::Ignored);
        t.handle(&ev(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(t.value, "");
    }

    #[test]
    fn text_input_handles_multibyte() {
        let mut t = TextInput::new("café");
        t.handle(&ev(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(t.value, "caf");
        t.handle(&key('é'));
        t.handle(&ev(KeyCode::Home, KeyModifiers::NONE));
        t.handle(&ev(KeyCode::Delete, KeyModifiers::NONE));
        assert_eq!(t.value, "afé");
    }
}
