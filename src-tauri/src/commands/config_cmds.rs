//! Global agent config commands (named `config_cmds` to avoid clashing with
//! the top-level `config` module). Reads/writes the non-secret agent config
//! and manages the OS-keychain auth token.

use super::state::AppState;
use crate::config::AgentConfig;
use crate::error::AppError;
use crate::secrets;
use tauri::State;

/// Get the global agent configuration.
///
/// Returns `None` if no agent config has been saved yet. The auth token is
/// **never** returned to the renderer: `auth_token` is always `None` here, and
/// `has_auth_token` reports whether one is stored in the OS keychain so the UI
/// can show a masked "token stored" affordance without the secret crossing IPC.
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
/// If a non-empty `auth_token` is supplied it is stored in the OS keychain
/// (overwriting any existing value); the token is never written to
/// `config.yaml`. An empty/`None` token means "leave the stored keychain token
/// untouched" — because `get_agent_config` never returns the plaintext token,
/// an unchanged Settings field shows up empty and must not be interpreted as a
/// request to clear. Use [`clear_auth_token`] to remove a stored token.
#[tauri::command]
pub async fn set_agent_config(
    agent_config: AgentConfig,
    state: State<'_, AppState>,
) -> Result<AgentConfig, AppError> {
    // Store a newly-typed token in the keychain; otherwise keep whatever is
    // already there.
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

    // Reflect actual keychain presence back to the caller (used by the UI to
    // flip the field into its "stored" state).
    persisted.has_auth_token = has_token;
    Ok(persisted)
}

/// Remove the stored auth token from the OS keychain.
///
/// Idempotent: succeeding when no token is stored. The non-secret agent config
/// (base_url / model / effort) in `config.yaml` is left untouched.
#[tauri::command]
pub async fn clear_auth_token() -> Result<(), AppError> {
    secrets::delete_auth_token()
}
