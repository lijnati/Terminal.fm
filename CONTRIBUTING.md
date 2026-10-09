# Contributing to Terminal.fm

Thanks for helping. Terminal.fm aims to stay small and focused. It's a great local music player,
and features like streaming, accounts and cloud sync are out of scope.

## Local setup

1. Install stable Rust with `rustup` (1.85 or newer).
2. On Linux, install the ALSA headers: `sudo apt install libasound2-dev pkg-config`
   (or your distribution's equivalent).
3. `cargo run -- ~/Music`, or `cargo run -- --no-audio <folder>` on machines without sound.

To keep your real config untouched while developing, point the app at a scratch directory:

```sh
TERMINALFM_HOME=/tmp/tfm-dev cargo run -- ~/Music
```

## Testing

```sh
cargo test --all-features
```

- None of the tests need an audio device. Engine tests use the null output
  (`EngineOptions { null_output: true }`), which pulls samples in real time without playing them.
- Codec fixtures live in `tests/fixtures`. Its README has the ffmpeg commands that generated them.
- Rendering tests use ratatui's `TestBackend`. A new view needs a test at small sizes
  (see `SIZES` in `src/ui/tests.rs`) to prove it never panics or overflows.
- Signal-processing changes need deterministic tests built from synthetic signals
  (silence, sines, mixes, short or invalid buffers).

## Code style

- `cargo fmt` (settings in `rustfmt.toml`) and `cargo clippy -- -D warnings` must both pass.
- Follow the layering rules in [CLAUDE.md](CLAUDE.md). Most importantly, `audio`, `analysis`,
  `library`, `playback` and `storage` must not depend on `ratatui`.
- Views use colors only through `ui::theme::Theme` tokens.
- Every key binding lives in `input::keymap`, so the help overlay stays accurate.
- Keep channels and buffers bounded, and never block the UI or audio threads.
- Error messages are short, plain sentences, with no Rust debug output.

## Pull requests

- Keep each PR focused on one change, and explain *why* in the description.
- Run all four verification commands from the README before you push.
- Include screenshots or `tmux capture-pane` output for visual changes, in at least one dark
  theme and one light theme and at a narrow width.
- Say what you tested and what you couldn't (for example, "no macOS machine available").
