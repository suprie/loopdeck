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
//!   logging never stalls the turn read loop. Retention is **bounded** to the
//!   last [`MAX_LOG_FILES`] daily files (pruned at startup and on each rollover
//!   via the appender's `max_log_files` cap); the Settings → Diagnostics panel
//!   surfaces the folder path, current usage, and the cap.
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

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Once;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// The application org/name used for `ProjectDirs`, mirrored from `config.rs`.
/// Keeping it here avoids a cross-module dependency just for the log path.
const APP_ORG: &str = "loopdeck";
const APP_NAME: &str = "LoopDeck";

/// Maximum number of daily log files to retain on disk.
///
/// Older daily files are pruned both at startup (during appender construction)
/// and on each daily rollover, by `tracing-appender`'s `max_log_files` cap.
/// Without this the daily roller grows forever — one file per day, never purged
/// — which is exactly the unbounded-growth hazard this module must avoid. 14
/// days keeps enough history to diagnose a recurring issue while bounding disk
/// use; the Settings → Diagnostics panel surfaces the cap and current usage so
/// the bound is visible, not magic. Override at your own risk — there is no
/// runtime knob by design (a config-driven cap would just be one more input to
/// validate and would rarely be changed).
pub const MAX_LOG_FILES: usize = 14;

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
        //
        // Built via the `Builder` API (not the legacy `rolling::daily` helper)
        // so we can set `max_log_files(MAX_LOG_FILES)` — that caps retention,
        // pruning old daily files at construction (now) and on each rollover.
        // The legacy helper has no retention knob, so the daily roller would
        // otherwise grow forever. `filename_prefix("loopdeck.log")` preserves
        // the existing `loopdeck.log.YYYY-MM-DD` naming so old files and the
        // documented path (`docs/alpha-distribution.md`) stay valid.
        let file_layer = match resolve_log_dir() {
            Ok(dir) => match rolling::RollingFileAppender::builder()
                .rotation(rolling::Rotation::DAILY)
                .filename_prefix("loopdeck.log")
                .max_log_files(MAX_LOG_FILES)
                .build(&dir)
            {
                Ok(file_appender) => {
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
                    // Appender build failed (e.g. the dir vanished between
                    // resolve and build). Degrade to stderr-only — logging is
                    // observability, never a startup blocker.
                    eprintln!(
                        "loopdeck: could not build rolling appender in {}, falling back to stderr-only: {e}",
                        dir.display()
                    );
                    None
                }
            },
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
/// successfully. Used by the `reveal_log_dir` command to open the folder in the
/// OS file manager, and by [`collect_log_info`] for the Diagnostics panel.
/// Returns the *directory*; the actual file has a `.YYYY-MM-DD` date suffix
/// appended by the rolling appender.
pub fn log_dir() -> Option<&'static std::path::Path> {
    LOG_DIR.get().map(std::path::Path::new)
}

// ── Diagnostics surface ─────────────────────────────────────────────────────

/// One retained log file, surfaced in the Settings → Diagnostics panel.
/// Mirrors the TS `LogFileInfo`.
#[derive(Debug, Clone, Serialize)]
pub struct LogFileInfo {
    /// File name, e.g. `loopdeck.log.2026-07-23`.
    pub name: String,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// Snapshot of the log directory for the Settings → Diagnostics panel.
/// Mirrors the TS `LogInfo`.
///
/// Returned by [`collect_log_info`]. `dir` is `None` when init fell back to
/// stderr-only (no log directory is in use); the UI then explains that logging
/// is stderr-only rather than offering an "open folder" affordance.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LogInfo {
    /// Absolute path to the log directory, when the file appender is active.
    pub dir: Option<String>,
    /// Retained `loopdeck.log*` files, newest-first. The daily date suffix is
    /// `YYYY-MM-DD`, which sorts lexically, so a descending name sort is a
    /// descending-date sort.
    pub files: Vec<LogFileInfo>,
    /// Total bytes across `files` (the live on-disk footprint).
    pub total_bytes: u64,
    /// The configured retention cap ([`MAX_LOG_FILES`]); surfaced so the UI can
    /// say "keeping the last N daily logs".
    pub max_files: usize,
}

/// Build a [`LogInfo`] snapshot of the current log directory.
///
/// Delegates to [`collect_log_info_in`] against the process-global `LOG_DIR`
/// resolved at init. Reads only file *names* and *sizes* — never file contents
/// — so the diagnostics surface can't exfiltrate whatever a future log line
/// might accidentally contain. Intended for the `get_log_info` command.
pub fn collect_log_info() -> LogInfo {
    let Some(dir) = LOG_DIR.get().map(PathBuf::from) else {
        // Stderr-only mode: no directory to describe.
        return LogInfo {
            dir: None,
            max_files: MAX_LOG_FILES,
            ..Default::default()
        };
    };
    collect_log_info_in(&dir)
}

/// Pure, testable core of [`collect_log_info`]: enumerate `loopdeck.log*` in
/// `dir` as a [`LogInfo`]. Exposed (not `pub`) so the unit tests can drive it
/// against a temp dir without touching the process-global `LOG_DIR`. A
/// directory-read error degrades to an empty file list rather than propagating
/// — diagnostics must never break the Settings panel.
fn collect_log_info_in(dir: &std::path::Path) -> LogInfo {
    let mut files: Vec<LogFileInfo> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                // Only the rolling appender's output: today's active file plus
                // any retained historical daily files. The `.YYYY-MM-DD` suffix
                // is part of the name, so `starts_with` is the right filter.
                if !name.starts_with("loopdeck.log") {
                    return None;
                }
                let size_bytes = e.metadata().ok().map(|m| m.len()).unwrap_or(0);
                Some(LogFileInfo { name, size_bytes })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    // Descending name sort = descending date for `loopdeck.log.YYYY-MM-DD`.
    files.sort_by(|a, b| b.name.cmp(&a.name));
    let total_bytes = files.iter().map(|f| f.size_bytes).sum();
    LogInfo {
        dir: Some(dir.to_string_lossy().into_owned()),
        files,
        total_bytes,
        max_files: MAX_LOG_FILES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Unique temp dir per test, keyed by name + PID + nanos so parallel test
    /// runs can't race on a shared parent. Mirrors the pattern in `persist.rs`.
    fn temp_dir(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-logging-{test_name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn collect_log_info_lists_only_loopdeck_logs_newest_first() {
        let dir = temp_dir("lists_logs");
        fs::write(dir.join("loopdeck.log.2026-07-22"), b"older").unwrap();
        fs::write(dir.join("loopdeck.log.2026-07-23"), b"today!").unwrap();
        fs::write(dir.join("README.txt"), b"ignore me").unwrap();
        fs::write(dir.join("loopdeck.log.bak"), b"bak").unwrap(); // prefix match

        let info = collect_log_info_in(&dir);

        assert_eq!(info.dir.as_deref(), Some(dir.to_str().unwrap()));
        // All three loopdeck.log* files (newest-first), README excluded.
        let names: Vec<&str> = info.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "loopdeck.log.bak",
                "loopdeck.log.2026-07-23",
                "loopdeck.log.2026-07-22",
            ]
        );
        assert_eq!(
            info.total_bytes,
            (b"older".len() + b"today!".len() + b"bak".len()) as u64
        );
        assert_eq!(info.max_files, MAX_LOG_FILES);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn collect_log_info_missing_dir_is_empty_not_error() {
        // A directory that does not exist must yield an empty file list, not a
        // panic — the Diagnostics panel must never break on a missing log dir.
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-logging-absent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!dir.exists());
        let info = collect_log_info_in(&dir);
        assert_eq!(info.dir.as_deref(), Some(dir.to_str().unwrap()));
        assert!(info.files.is_empty());
        assert_eq!(info.total_bytes, 0);
        assert_eq!(info.max_files, MAX_LOG_FILES);
    }

    /// End-to-end proof that the configured `max_log_files` cap actually bounds
    /// retention with this `tracing-appender` version. Seeds more files than the
    /// cap, builds an appender (which prunes at construction), and asserts the
    /// on-disk count is at-or-below the cap. Without the cap the count would be
    /// `seeded + 1` (today); with it, the appender deletes down before opening
    /// today's file.
    #[test]
    fn rolling_appender_max_log_files_caps_retention() {
        let dir = temp_dir("retention_cap");
        // 12 daily files, dated in a month well away from "today" so none
        // collides with the file `build()` opens for the current date.
        for day in 1..=12u32 {
            let name = format!("loopdeck.log.2026-01-{day:02}");
            fs::write(dir.join(name), b"x").unwrap();
        }

        let appender = rolling::RollingFileAppender::builder()
            .rotation(rolling::Rotation::DAILY)
            .filename_prefix("loopdeck.log")
            .max_log_files(7)
            .build(&dir)
            .expect("appender builds");
        drop(appender); // flush + close; pruning already ran in build()

        let remaining = collect_log_info_in(&dir);
        // The guarantee the product gives users: at most `max_log_files` daily
        // logs on disk. 12 seeded + today's open would be 13 without pruning;
        // the cap must bring it down to <= 7.
        assert!(
            remaining.files.len() <= 7,
            "retention cap not enforced: {} files remain ({})",
            remaining.files.len(),
            remaining
                .files
                .iter()
                .map(|f| f.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Pins the invariant the logger depends on: `tracing` formats recorded
    /// values with their `Debug` impl (the `?` / `{:?}` field syntax), so an
    /// `AgentConfig` carrying a token must never render the token through
    /// `Debug`. If someone replaces the manual redacting impl with a derived
    /// one, this breaks loudly. This is the "confirm no auth_token is ever
    /// logged" guarantee — the only structural path from a token to a log line.
    #[test]
    fn agent_config_debug_never_renders_token() {
        use crate::config::AgentConfig;
        let agent = AgentConfig {
            auth_token: Some("sk-logging-invariant-secret".into()),
            base_url: Some("https://example.test".into()),
            ..Default::default()
        };
        let rendered = format!("{agent:?}");
        assert!(
            !rendered.contains("sk-logging-invariant-secret"),
            "token leaked via Debug: {rendered}"
        );
        assert!(rendered.contains("***REDACTED***"));
    }
}
