//! Process-wide tracing initialization: rolling file appender + stderr, with
//! the `log` crate bridged in via `tracing-subscriber`'s built-in `tracing-log`
//! feature.
//!
//! Before this module existed, every `tracing::info!`/`debug!`/`warn!` call in
//! the codebase was a no-op — there was no subscriber installed, so events went
//! nowhere. This installs one subscriber globally at startup with two layers:
//!
//! - **File** (`tracing-appender::rolling`): one log file per day under the
//!   platform's log dir (`~/Library/Logs/LoopDeck/` on macOS, etc.), named
//!   `loopdeck.log.YYYY-MM-DD`. Non-blocking — a background worker flushes, so
//!   logging never stalls the turn read loop. Kept across restarts (rolled
//!   daily, not purged — manage manually if it grows).
//! - **Stderr** (`fmt` layer): the same events to stderr for `RUST_LOG`-style
//!   debugging and for capturing via `tauri dev`'s terminal.
//!
//! The `log`-crate bridge is wired up by `tracing-subscriber` itself: its
//! `tracing-log` feature is on by default, so `.try_init()` installs the
//! `LogTracer` as part of setting the global subscriber. We must NOT call
//! `tracing_log::LogTracer::init()` ourselves — doing so sets the `log` global
//! logger first, and then `.try_init()`'s internal bridge call fails with
//! `SetLoggerError` (the startup panic this code path caused before). One
//! owner, no race.
//!
//! **Filtering.** Driven by the `RUST_LOG` env var when set; otherwise defaults
//! to `info` for our crate + `warn` for deps. The NDJSON lines streamed from
//! the claude process are logged at `debug` under the `loopdeck::claude_wire`
//! target (see `claude_session.rs`), so set `RUST_LOG=loopdeck=debug` to
//! capture the raw wire traffic when debugging the control protocol or a
//! stalled turn.
//!
//! **Init order.** Must be the FIRST thing `run()` does, before any Tauri
//! plugin loads — otherwise those plugins' early `log` records would be missed
//! (no subscriber yet to receive them). Guarded by `Once` so re-calling is a
//! no-op (defensive; the subscriber is global anyway).

use std::path::PathBuf;
use std::sync::Once;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// The application org/name used for `ProjectDirs`, mirrored from `config.rs`.
/// Keeping it here avoids a cross-module dependency just for the log path.
const APP_ORG: &str = "loopdeck";
const APP_NAME: &str = "LoopDeck";

/// The default filter applied when `RUST_LOG` is unset.
///
/// `info` for our own crate captures the permission decisions, AskUserQuestion
/// parking, session lifecycle, and command entrypoints that are useful day-to-
/// day. Deps default to `warn` so serde/tokio/tauri don't flood the file; bump
/// with `RUST_LOG=loopdeck=debug,tokio=warn` when chasing something specific.
/// The raw NDJSON wire lines (received from / written to the claude process)
/// are logged at `debug` under target `loopdeck::claude_wire`.
const DEFAULT_FILTER: &str = "loopdeck=info,warn";

static INIT: Once = Once::new();

/// The resolved log directory, recorded at init time so `log_dir()` can surface
/// it later (e.g. in a Settings panel). Set once inside `init_logging`; read
/// thereafter. A plain `OnceLock<PathBuf>` — no `unsafe`.
static LOG_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Initialize process-wide tracing. Idempotent (guarded by `Once`).
///
/// Installs the file + stderr layers and bridges `log`. On failure to resolve a
/// log directory, degrades gracefully to stderr-only — logging is observability,
/// not correctness, so it must never prevent startup.
///
/// **The file appender's `WorkerGuard` is intentionally leaked** (via
/// `Box::leak`) so it lives for the whole process. The guard's `Drop` flushes
/// the background writer; we never want that to run until process exit, so we
/// hold it forever. This is the documented pattern for `tracing-appender`.
pub fn init_logging() {
    INIT.call_once(|| {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

        // Stderr layer — always installed. Useful under `tauri dev` and when
        // capturing from a terminal; mirrors the file content.
        let stderr_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .with_thread_names(false)
            .with_ansi(false); // no color codes in piped/logged output

        // File layer — rolling daily under the platform log dir. Wrapped in
        // `non_blocking` so a slow disk can't stall the turn read loop. The
        // guard is leaked (see the function doc) so it flushes only at exit.
        let file_layer = match resolve_log_dir() {
            Ok(dir) => {
                let file_appender = rolling::daily(&dir, "loopdeck.log");
                let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                // Leak the guard so it (and its background worker) live for the
                // process lifetime. Dropping it would flush + shut the writer,
                // dropping any events still in flight — we don't want that
                // until the OS reaps us. A tiny per-process leak, by design.
                let _ = Box::leak(Box::new(guard));
                let _ = LOG_DIR.set(dir.clone());
                Some(
                    fmt::layer()
                        .with_ansi(false)
                        .with_target(true)
                        .with_writer(non_blocking),
                )
            }
            Err(e) => {
                // Fall back to stderr-only. Use eprintln here (not tracing) —
                // the subscriber isn't installed yet, so a tracing macro would
                // be dropped.
                eprintln!(
                    "loopdeck: could not resolve log directory, falling back to stderr-only: {e}"
                );
                None
            }
        };

        // Install as the GLOBAL default. We do NOT call `tracing_log::LogTracer::init()`
        // ourselves: `tracing-subscriber` enables its `tracing-log` feature by
        // default, so `.try_init()` already wires up the `log`-crate bridge as
        // part of installing the subscriber. Calling `LogTracer::init()` first
        // sets the `log` global logger, and then `.init()`'s internal bridge
        // call fails with `SetLoggerError` — the exact panic this code path
        // caused before. Letting `.try_init()` own both avoids the race.
        //
        // `try_init` (not `init`) so a competing early subscriber — e.g. a test
        // harness or a Tauri plugin that sneaks one in first — degrades
        // gracefully to stderr instead of panicking. Logging is observability:
        // it must never prevent the app from running.
        if let Err(e) = tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .with(file_layer)
            .try_init()
        {
            eprintln!(
                "loopdeck: tracing subscriber already installed; file logging disabled. ({e})"
            );
            return;
        }

        if let Some(path) = LOG_DIR.get() {
            tracing::info!(log_dir = %path.display(), "logging initialized");
        } else {
            tracing::info!("logging initialized (stderr-only)");
        }
    });
}

/// Resolve the platform log directory for LoopDeck.
///
/// Each OS has its own convention for where apps write logs:
/// - **macOS**: `~/Library/Logs/LoopDeck/` (Console.app and `log show` look here)
/// - **Linux**: `$XDG_STATE_HOME/loopdeck` (or `~/.local/state/loopdeck`)
/// - **Windows**: `%LOCALAPPDATA%\loopdeck\logs`
///
/// `directories` v6 dropped `ProjectDirs::log_dir()`, so we build the path from
/// `BaseDirs::home_dir()` + the platform's conventional subdir. Falls back to a
/// `./logs/` directory adjacent to the working directory if HOME can't be
/// resolved (vanishingly rare — headless/containers).
fn resolve_log_dir() -> Result<PathBuf, String> {
    // Try the platform convention first; fall back to ./logs (relative) so we
    // always have *somewhere* to write, even on a headless box without HOME.
    let dir = std::env::var_os("LOOPDECK_LOG_DIR")
        .map(PathBuf::from)
        .or_else(platform_log_dir)
        .unwrap_or_else(|| PathBuf::from("./logs"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("create log dir {}: {e}", dir.display()))?;
    Ok(dir)
}

/// The platform-conventional log directory for LoopDeck, or `None` when the
/// home directory can't be resolved.
fn platform_log_dir() -> Option<PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Logs").join(APP_NAME)
    } else if cfg!(target_os = "windows") {
        // %LOCALAPPDATA% lives under home on single-user boxes; if not, fall
        // back to home/LocalAppData.
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join(APP_ORG)
            .join("logs")
    } else {
        // Linux / other Unix: XDG state dir, falling back to ~/.local/state.
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"))
            .join(APP_ORG)
    })
}

/// The log directory currently in use, if the file appender installed
/// successfully. Intended for surfacing in a future Settings panel so the user
/// can open/reveal the log file. Returns the *directory*; the actual file has a
/// `.YYYY-MM-DD` date suffix appended by the rolling appender.
#[allow(dead_code)] // surfaced for future UI; not yet wired up
pub fn log_dir() -> Option<&'static std::path::Path> {
    LOG_DIR.get().map(std::path::Path::new)
}
