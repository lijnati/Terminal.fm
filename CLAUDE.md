# Terminal.fm — project conventions

Keyboard-first terminal music player in Rust (ratatui + rodio/symphonia). Offline only.

## Verification (run before every commit)

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release
```

Linux builds need ALSA headers: `sudo apt-get install libasound2-dev pkg-config`.

## Architecture rules

- Layering: `ui → app → {audio, analysis, library, playback, storage}`.
  `audio`, `analysis`, `library`, `playback`, `storage` must never import `ratatui`.
- `playback` (queue/shuffle/repeat) and `analysis` are pure and deterministic — keep them that way.
- The audio engine runs on its own thread; the UI talks to it only via `audio::Command`
  (bounded `sync_channel`) and reads `audio::Event` / atomic status. Never block the UI thread
  on decoding or filesystem work. Library scans run on a worker thread.
- The visualizer consumes the **same decoded PCM** that is played, via `audio::tap`
  (lock-free SPSC ring; drop-on-full; never block the audio thread). Never synthesize
  fake motion: no samples ⇒ show an honest "no signal"/paused state.
- All channels and buffers are bounded.
- Colors only come from `ui::theme::Theme` semantic tokens — no hardcoded `Color::` in views.
- Rendering must never panic at any terminal size; layout math uses saturating ops.
  Add a `TestBackend` test for any new view at small sizes.
- Keyboard: every binding lives in `input::keymap`; the help overlay is generated from it.
  Text-entry contexts must not trigger playback shortcuts.

## Persistence

- `config.toml` (user-edited, TOML). Invalid config ⇒ warn + defaults, never overwrite the file.
- `state.json` (favorites, playlists, recents, last volume/visualizer). Atomic writes.
- `library.json` cache (rebuildable). Paths from `storage::paths::AppPaths`;
  `TERMINALFM_HOME` overrides all locations (used by tests and captures).

## Testing

- Unit tests live beside code. Integration tests in `tests/` use the null audio output
  (`EngineOptions { null_output: true }`) — no audio device required.
- Codec fixtures in `tests/fixtures` (see its README); WAVs for timing tests are generated
  with `hound` at test time.
- Don't claim something works without a test or a captured run. Real speaker output cannot be
  verified in CI/containers — say so.

## Style

- `rustfmt.toml` sets `max_width = 110`. Prefer small modules with explicit state transitions.
- User-facing errors are short sentences; no Rust debug output or backtraces for normal failures.
