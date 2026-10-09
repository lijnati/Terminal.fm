//! Command-line interface and startup.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::app::{App, AppInit, validate_dir};
use crate::audio::EngineOptions;
use crate::library::Library;
use crate::library::scan::{ScanProgress, scan};
use crate::storage::{AppPaths, Config, ConfigLoad, LibraryCache, State};
use crate::ui::theme::ColorDepth;

#[derive(Debug, Parser)]
#[command(
    name = "terminalfm",
    version,
    about = "Your terminal. Your music. Your vibe.",
    long_about = "Terminal.fm — a keyboard-first music player with a real-time audio visualizer.\n\
                  Runs entirely offline. Press ? inside the app for keyboard shortcuts.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Music folder to add to the library and open.
    pub path: Option<PathBuf>,

    /// Don't open an audio device (the player and visualizer run silently).
    #[arg(long)]
    pub no_audio: bool,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Rescan the configured music folders and update the library.
    Scan,
    /// Show where the configuration and data files live.
    Config,
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("terminalfm: {msg}");
    ExitCode::from(2)
}

fn init_logging(paths: &AppPaths) {
    use tracing_subscriber::EnvFilter;
    let Some(dir) = paths.log_file.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&paths.log_file) else {
        return;
    };
    let filter = EnvFilter::try_from_env("TERMINALFM_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(false)
        .with_writer(std::sync::Mutex::new(file))
        .try_init();
}

/// Panics are bugs: log the details and print one friendly line instead of a backtrace.
fn install_panic_hook(log: Option<PathBuf>) {
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".into());
        tracing::error!("panic at {location}: {msg}\n{}", std::backtrace::Backtrace::force_capture());
        let mut err = std::io::stderr();
        let _ = writeln!(err, "\nTerminal.fm hit an unexpected error and had to close: {msg}");
        match &log {
            Some(p) => {
                let _ = writeln!(err, "Details were written to {}", p.display());
            }
            None => {
                let _ = writeln!(err, "at {location}");
            }
        }
    }));
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let Some(paths) = AppPaths::resolve() else {
        return fail("could not determine a home directory; set TERMINALFM_HOME to a writable folder");
    };
    init_logging(&paths);
    install_panic_hook(Some(paths.log_file.clone()));

    let (mut config, config_writable, first_run, mut warnings) = match Config::load(&paths.config_file) {
        ConfigLoad::Missing => (Config::default(), true, true, Vec::new()),
        ConfigLoad::Loaded(c) => (c, true, false, Vec::new()),
        ConfigLoad::Invalid(e) => (Config::default(), false, false, vec![format!("Using defaults — {e}")]),
    };

    match cli.command {
        Some(CliCommand::Config) => {
            println!("config   {}", paths.config_file.display());
            println!("state    {}", paths.state_file.display());
            println!("library  {}", paths.library_cache.display());
            println!("log      {}", paths.log_file.display());
            if let Some(w) = warnings.first() {
                eprintln!("\nwarning: {w}");
            }
            return ExitCode::SUCCESS;
        }
        Some(CliCommand::Scan) => return scan_command(&paths, &config),
        None => {}
    }

    let mut first_run = first_run;
    if let Some(p) = &cli.path {
        let dir = match validate_dir(&p.to_string_lossy()) {
            Ok(d) => d,
            Err(e) => return fail(format!("{}: {e}", p.display())),
        };
        if !config.music_dirs.contains(&dir) {
            config.music_dirs.push(dir);
            if config_writable {
                if let Err(e) = config.save(&paths.config_file) {
                    warnings.push(format!("Couldn't save config: {e}"));
                }
            }
        }
        first_run = false;
    }

    let (mut state, state_warning) = State::load(&paths.state_file);
    warnings.extend(state_warning);
    let cache = LibraryCache::load(&paths.library_cache).filter(|c| c.roots == config.music_dirs);
    let had_cache = cache.is_some();
    let library = Library::new(cache.map(|c| c.tracks).unwrap_or_default());
    state.relink(&library);

    let mut app = App::new(AppInit {
        config,
        config_writable,
        first_run,
        paths: Some(paths.clone()),
        state,
        library,
        engine: EngineOptions { null_output: cli.no_audio },
        color_depth: ColorDepth::detect(),
        warnings,
    });
    if !first_run && !app.config.music_dirs.is_empty() {
        // Refresh in the background; unchanged files are reused from the cache.
        app.start_scan(!had_cache);
        if !had_cache {
            app.screen = crate::app::Screen::Library;
        }
    }

    match crate::app::run::run(&mut app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("terminal error: {e}");
            fail(format!("terminal error: {e}"))
        }
    }
}

fn scan_command(paths: &AppPaths, config: &Config) -> ExitCode {
    if config.music_dirs.is_empty() {
        return fail("no music folders configured — run `terminalfm <folder>` first");
    }
    let previous = LibraryCache::load(&paths.library_cache).map(|c| c.tracks).unwrap_or_default();
    for d in &config.music_dirs {
        println!("Scanning {}", d.display());
    }
    let mut err = std::io::stderr();
    let result = scan(&config.music_dirs, &previous, |p| {
        if let ScanProgress::Read { done, total } = p {
            let _ = write!(err, "\r  {done}/{total} files");
            let _ = err.flush();
        }
    });
    let _ = writeln!(err);
    for (root, e) in &result.unreadable_roots {
        eprintln!("  can't read {}: {e}", root.display());
    }
    for (path, e) in &result.failures {
        eprintln!("  skipped {}: {e}", path.display());
    }
    let library = Library::new(result.tracks);
    let cache = LibraryCache { roots: config.music_dirs.clone(), tracks: library.tracks().to_vec() };
    if let Err(e) = cache.save(&paths.library_cache) {
        return fail(format!("couldn't write {}: {e}", paths.library_cache.display()));
    }
    let (mut state, _) = State::load(&paths.state_file);
    if state.relink(&library) {
        let _ = state.save(&paths.state_file);
    }
    println!(
        "Library: {} tracks · {} artists · {} albums{}",
        library.len(),
        library.artists().len(),
        library.albums().len(),
        if result.failures.is_empty() {
            String::new()
        } else {
            format!(" · {} skipped", result.failures.len())
        }
    );
    ExitCode::SUCCESS
}
