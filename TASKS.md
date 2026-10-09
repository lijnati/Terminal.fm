# TASKS

Implementation checklist. `[x]` = implemented **and** verified by tests or a captured run.

## Foundation
- [x] Cargo scaffold, `terminalfm` + `tfm` binaries, release profile
- [x] CLAUDE.md, TASKS.md, .gitignore, rustfmt.toml
- [x] CI workflow written (fmt, clippy, test on Linux/macOS/Windows) — not yet run on GitHub

## Core
- [x] Track model, metadata normalization + filename/folder fallbacks
- [x] Recursive scanner (walkdir + lofty), corrupt-file reporting, mtime cache reuse
- [x] Library grouping (artists/albums/genres), sorting, ranked accent-insensitive search
- [x] Queue: next/prev, shuffle (seeded, current-first), repeat off/all/one, remove, remap
- [x] Config (TOML) with validation/clamping; example config kept valid by a test
- [x] State (JSON): favorites, playlists CRUD + reorder, recents (bounded), relink moved files
- [x] Atomic writes; corrupt state backed up and reset

## Audio
- [x] Engine thread with bounded command channel, events, atomic position/state
- [x] Tap source → lock-free ring → visualizer (pre-volume, drop-on-full)
- [x] Null realtime output when no device; ALSA stderr noise suppressed during probing
- [x] Verified formats: MP3, FLAC, OGG Vorbis, AAC/M4A, WAV (tests/formats.rs)
- [x] Corrupt / missing / unsupported files fail cleanly
- [ ] Real speaker output (no audio device in dev container — unverified)

## Analysis
- [x] Hann-windowed 2048-pt FFT, log bands, interpolation for narrow low bands, dB normalization
- [x] Attack/release smoothing, peak hold + decay, sensitivity
- [x] Waveform with zero-crossing trigger + adaptive gain; RMS/peak levels
- [x] Signal states: none / silence / active (no fake motion)

## UI
- [x] Terminal lifecycle (raw mode, alt screen, panic restore)
- [x] Theme tokens: Mono, Midnight, Amber, Paper
- [x] Responsive layouts (full / standard / compact / tiny)
- [x] Welcome / first-run flow with directory input + scan progress
- [x] Now Playing (track info, visualizer, progress, controls, queue preview)
- [x] Library (tracks/artists/albums/genres), sorting
- [x] Search
- [x] Playlists (create/rename/delete, add/remove/reorder)
- [x] Favorites, Recently played
- [x] Settings
- [x] Help overlay generated from keymap
- [x] Visualizer renderers: spectrum, waveform, minimal, off

## CLI
- [x] `terminalfm [PATH]`, `scan`, `config`, `--help`, `--version`, `--no-audio`

## Docs & release prep
- [x] README, CONTRIBUTING, LICENSE-MIT, LICENSE-APACHE
- [x] Final verification pass + terminal captures

## Known gaps / next
- [ ] Real speaker output, macOS and Windows: unverified (no device / platforms in dev container)
- [ ] SIGTERM/SIGHUP handler to restore the terminal when killed
- [ ] Two-column help overlay on wide terminals
- [ ] Gapless playback; ReplayGain
- [ ] Opus / ALAC (needs extra symphonia features + fixtures)
- [ ] Library file watching instead of rescan-on-start
