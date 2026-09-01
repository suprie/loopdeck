//! Global, non-project-scoped commands (named `config_cmds` to avoid clashing
//! with the top-level `config` module). Three concerns live here because none
//! of them is tied to a registered project root: the non-secret agent config,
//! the local-secrets-file auth token, and user-accessible diagnostics (the log
//! directory snapshot + "reveal in Finder").

use super::state::AppState;
use crate::config::{AgentConfig, GlobalConfig, NamedAgentConfig, RoleCharter};
use crate::error::AppError;
use crate::logging;
use crate::secrets;
use tauri::State;

fn response_agent(mut agent: NamedAgentConfig) -> Result<NamedAgentConfig, AppError> {
    agent.config.auth_token = None;
    agent.config.has_auth_token = secrets::load_agent_auth_token(&agent.id)?.is_some();
    Ok(agent)
}

/// Restore one UUID-scoped secret to its pre-mutation state. This is used only
/// when the registry save fails after a credential write/delete has succeeded:
/// registry and secret then return to the same in-memory state.
fn restore_agent_secret(id: &str, previous: &Option<String>) -> Result<(), AppError> {
    match previous {
        Some(token) => secrets::store_agent_auth_token(id, token),
        None => secrets::delete_agent_auth_token(id),
    }
}

fn rollback_after_save_failure(
    config: &mut GlobalConfig,
    before: GlobalConfig,
    id: Option<&str>,
    previous_secret: Option<&Option<String>>,
    save_error: AppError,
) -> AppError {
    *config = before;
    if let (Some(id), Some(previous_secret)) = (id, previous_secret) {
        if let Err(rollback_error) = restore_agent_secret(id, previous_secret) {
            return AppError::Config(format!(
                "failed to save agent roster: {save_error}; credential rollback for '{id}' also failed: {rollback_error}"
            ));
        }
    }
    save_error
}

/// List the global named-agent roster. Tokens are never returned over IPC;
/// each entry exposes only whether its UUID-scoped secret is present.
#[tauri::command]
pub async fn list_agent_configs(
    state: State<'_, AppState>,
) -> Result<Vec<NamedAgentConfig>, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    config
        .agents
        .clone()
        .into_iter()
        .map(response_agent)
        .collect()
}

/// Create a named agent config. `name` must be non-empty and unique without
/// regard to case. The generated UUID is returned, never accepted from
/// the renderer.
#[tauri::command]
pub async fn create_agent_config(
    name: String,
    agent_config: AgentConfig,
    state: State<'_, AppState>,
) -> Result<NamedAgentConfig, AppError> {
    let token = agent_config
        .auth_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let before = config.clone();
    let created = config.create_agent_config(name, agent_config)?;
    let no_previous_secret = None;
    if let Some(token) = token.as_deref() {
        if let Err(error) = secrets::store_agent_auth_token(&created.id, token) {
            *config = before;
            return Err(error);
        }
    }
    if let Err(error) = config.save() {
        return Err(rollback_after_save_failure(
            &mut config,
            before,
            token.as_deref().map(|_| created.id.as_str()),
            token.as_ref().map(|_| &no_previous_secret),
            error,
        ));
    }
    response_agent(created)
}

/// Update a named entry while preserving its immutable UUID. Empty/missing
/// `auth_token` leaves the existing UUID-scoped token untouched.
#[tauri::command]
pub async fn update_agent_config(
    id: String,
    name: String,
    agent_config: AgentConfig,
    state: State<'_, AppState>,
) -> Result<NamedAgentConfig, AppError> {
    let token = agent_config
        .auth_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let before = config.clone();
    let existing = config
        .find_agent_config(&id)
        .cloned()
        .ok_or_else(|| AppError::Config(format!("agent config '{id}' was not found")))?;
    let previous_secret = token
        .as_ref()
        .map(|_| secrets::load_agent_auth_token(&id))
        .transpose()?;
    let updated = config.update_agent_config(&id, name, agent_config)?;
    if let Some(token) = token.as_deref() {
        if let Err(error) = secrets::store_agent_auth_token(&id, token) {
            *config = before;
            return Err(error);
        }
    }
    if let Err(error) = config.save() {
        return Err(rollback_after_save_failure(
            &mut config,
            before,
            token.as_deref().map(|_| existing.id.as_str()),
            previous_secret.as_ref(),
            error,
        ));
    }
    response_agent(updated)
}

/// Replace a named entry's role charter (persona prompt, allowed skills,
/// output contract). Advisory data only: nothing downstream parses or
/// enforces it in this phase. Missing/empty fields clear; connection
/// settings and the UUID are untouched. No secrets are involved, so the
/// rollback after a failed save is purely in-memory.
#[tauri::command]
pub async fn update_agent_charter(
    id: String,
    charter: RoleCharter,
    state: State<'_, AppState>,
) -> Result<NamedAgentConfig, AppError> {
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let before = config.clone();
    let updated = config.update_agent_charter(&id, charter.normalized())?;
    if let Err(error) = config.save() {
        *config = before;
        return Err(error);
    }
    response_agent(updated)
}

/// Delete a named entry and its UUID-scoped token. Deleting the default moves
/// the default to the oldest remaining roster entry.
#[tauri::command]
pub async fn delete_agent_config(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let before = config.clone();
    let existing = config
        .find_agent_config(&id)
        .cloned()
        .ok_or_else(|| AppError::Config(format!("agent config '{id}' was not found")))?;
    let previous_secret = secrets::load_agent_auth_token(&existing.id)?;
    secrets::delete_agent_auth_token(&existing.id)?;
    config.delete_agent_config(&id)?;
    if let Err(error) = config.save() {
        return Err(rollback_after_save_failure(
            &mut config,
            before,
            Some(existing.id.as_str()),
            Some(&previous_secret),
            error,
        ));
    }
    Ok(())
}

/// Read the roster entry backing legacy single-agent paths.
#[tauri::command]
pub async fn get_default_agent_config(
    state: State<'_, AppState>,
) -> Result<Option<NamedAgentConfig>, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    config
        .default_named_agent_config()
        .cloned()
        .map(response_agent)
        .transpose()
}

/// Select the roster entry used by single-agent commands.
#[tauri::command]
pub async fn set_default_agent_config(
    id: String,
    state: State<'_, AppState>,
) -> Result<NamedAgentConfig, AppError> {
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let before = config.clone();
    let agent = config.set_default_agent_config(&id)?;
    if let Err(error) = config.save() {
        return Err(rollback_after_save_failure(
            &mut config,
            before,
            None,
            None,
            error,
        ));
    }
    response_agent(agent)
}

/// Legacy single-config getter. It resolves the selected roster entry and
/// preserves the old flat response shape for existing settings clients.
#[tauri::command]
pub async fn get_agent_config(state: State<'_, AppState>) -> Result<Option<AgentConfig>, AppError> {
    Ok(get_default_agent_config(state)
        .await?
        .map(|agent| agent.config))
}

/// Legacy single-config setter. It creates the first `Default` entry or
/// updates the selected default entry without changing its UUID or name.
#[tauri::command]
pub async fn set_agent_config(
    agent_config: AgentConfig,
    state: State<'_, AppState>,
) -> Result<AgentConfig, AppError> {
    let token = agent_config
        .auth_token
        .as_deref()
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let before = config.clone();
    let existing = config.default_named_agent_config().cloned();
    let previous_secret = match (token.as_ref(), existing.as_ref()) {
        (Some(_), Some(existing)) => Some(secrets::load_agent_auth_token(&existing.id)?),
        _ => None,
    };
    let named = match existing {
        Some(existing) => config.update_agent_config(&existing.id, existing.name, agent_config)?,
        None => config.create_agent_config("Default".into(), agent_config)?,
    };
    if let Some(token) = token.as_deref() {
        if let Err(error) = secrets::store_agent_auth_token(&named.id, token) {
            *config = before;
            return Err(error);
        }
    }
    if let Err(error) = config.save() {
        return Err(rollback_after_save_failure(
            &mut config,
            before,
            token.as_deref().map(|_| named.id.as_str()),
            previous_secret.as_ref(),
            error,
        ));
    }
    Ok(response_agent(named)?.config)
}

/// Legacy default-entry token clear shim. The selected roster entry's token is
/// removed; if no roster exists the historical singleton file is cleared.
#[tauri::command]
pub async fn clear_auth_token(state: State<'_, AppState>) -> Result<(), AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    if let Some(agent) = config.default_named_agent_config() {
        secrets::delete_agent_auth_token(&agent.id)
    } else {
        secrets::delete_auth_token()
    }
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
