//! Global, non-project-scoped commands (named `config_cmds` to avoid clashing
//! with the top-level `config` module). Three concerns live here because none
//! of them is tied to a registered project root: the non-secret agent config,
//! the local-secrets-file auth token, and user-accessible diagnostics (the log
//! directory snapshot + "reveal in Finder").

use super::state::AppState;
use crate::config::AgentConfig;
use crate::error::AppError;
use crate::logging;
use crate::secrets;
use tauri::State;

/// Get the global agent configuration.
///
/// Returns `None` if no agent config has been saved yet. The auth token is
/// **never** returned to the renderer: `auth_token` is always `None` here, and
/// `has_auth_token` reports whether one is stored in the local secrets file so
/// the UI can show a masked "token stored" affordance without the secret
/// crossing IPC.
#[tauri::command]
pub async fn get_agent_config(state: State<'_, AppState>) -> Result<Option<AgentConfig>, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    let Some(mut agent) = config.agent.clone() else {
        return Ok(None);
    };
    agent.auth_token = None;
    agent.has_auth_token = secrets::load_auth_token()?.is_some();
    Ok(Some(agent))
}

/// Set (create or update) the global agent configuration.
///
/// If a non-empty `auth_token` is supplied it is stored in the local secrets
/// file (overwriting any existing value); the token is never written to
/// `config.yaml`. An empty/`None` token means "leave the stored token
/// untouched" — because `get_agent_config` never returns the plaintext token,
/// an unchanged Settings field shows up empty and must not be interpreted as a
/// request to clear. Use [`clear_auth_token`] to remove a stored token.
#[tauri::command]
pub async fn set_agent_config(
    agent_config: AgentConfig,
    state: State<'_, AppState>,
) -> Result<AgentConfig, AppError> {
    // Store a newly-typed token in the secrets file; otherwise keep whatever
    // is already there.
    let has_token = if let Some(token) = agent_config.auth_token.as_ref().filter(|t| !t.is_empty())
    {
        secrets::store_auth_token(token)?;
        true
    } else {
        secrets::load_auth_token()?.is_some()
    };

    // Persist only the non-secret fields. The token and the presence flag are
    // scrubbed so they never reach config.yaml.
    let mut persisted = agent_config.clone();
    persisted.auth_token = None;
    persisted.has_auth_token = false;

    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        config.agent = Some(persisted.clone());
        config.save()?;
    }

    // Reflect actual secrets-file presence back to the caller (used by the UI
    // to flip the field into its "stored" state).
    persisted.has_auth_token = has_token;
    Ok(persisted)
}

/// Remove the stored auth token from the local secrets file.
///
/// Idempotent: succeeding when no token is stored. The non-secret agent config
/// (base_url / model / effort) in `config.yaml` is left untouched.
#[tauri::command]
pub async fn clear_auth_token() -> Result<(), AppError> {
    secrets::delete_auth_token()
}

// ── Diagnostics ─────────────────────────────────────────────────────────────

/// Return a snapshot of the log directory for the Settings → Diagnostics panel:
/// the folder path, the retained log files + their sizes, the total size, and
/// the retention cap. Reads file *names/sizes only* — never contents — so the
/// panel can't exfiltrate whatever a future log line might contain. Never fails
/// (a missing/unreadable dir degrades to an empty file list); `dir` is `None`
/// when logging fell back to stderr-only at startup.
#[tauri::command]
pub async fn get_log_info() -> Result<logging::LogInfo, AppError> {
    Ok(logging::collect_log_info())
}

/// Open the log directory in the OS file manager (Finder on macOS) so the user
/// can inspect or share diagnostics. Returns a structured error when logging is
/// stderr-only (no folder exists) or the directory has vanished since startup —
/// the panel renders those as plain text rather than offering the button.
#[tauri::command]
pub async fn reveal_log_dir() -> Result<(), AppError> {
    let dir = logging::log_dir()
        .ok_or_else(|| AppError::Config("log directory unavailable (stderr-only mode)".into()))?
        .to_path_buf();
    if !dir.exists() {
        return Err(AppError::Config(format!(
            "log directory no longer exists: {}",
            dir.display()
        )));
    }
    open_dir_in_file_manager(&dir)
}

/// Spawn the OS file manager on `dir`. The path is app-controlled (resolved
/// from the logging module, never user input), so it needs none of the
/// project-boundary containment `open_in_finder` applies — but it is still
/// passed as a single argv element (never interpolated into a shell) so a path
/// can't be reinterpreted as a URL or scheme handler. Mirrors the
/// platform-fanout in `commands::project::open_in_finder`.
fn open_dir_in_file_manager(dir: &std::path::Path) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Config(format!("failed to open log folder: {e}")))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Config(format!("failed to open log folder: {e}")))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Config(format!("failed to open log folder: {e}")))?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = dir; // unused on unsupported platforms
        return Err(AppError::Config(
            "opening the log folder is unsupported on this platform".into(),
        ));
    }
    Ok(())
}
