//! Local file storage for the agent auth token.
//!
//! The auth token used to live in plaintext at `~/.config/loopdeck/config.yaml`,
//! then in the platform-native credential store (the `keyring` crate). The
//! keychain path was abandoned because an **unsigned/un-notarized** app is not
//! trusted by macOS, so every keychain access re-prompts the user for their
//! password — and the token is re-read on every agent spawn, so the prompt
//! appeared on essentially every message.
//!
//! The token now lives in a dedicated, owner-only file next to the registry:
//!
//! - `<config_dir>/agent_token` — `~/Library/Application Support/com.loopdeck.LoopDeck/`
//!   on macOS, `~/.config/loopdeck/` on Linux / as the headless fallback.
//!
//! The file is written crash-safe via [`crate::persist::atomic_write`] (temp +
//! fsync + same-dir rename) and locked down to `0600` (owner-only) on Unix as a
//! permission floor. It is kept separate from `config.yaml` so the secret is not
//! mixed into the registry that gets inspected, backed up, or shared for
//! debugging. The rest of the agent config (`base_url`, `model`, `effort`) is
//! non-secret and stays in the YAML file.
//!
//! Failures degrade gracefully: a missing/empty token file is reported as
//! `Ok(None)` (treated identically to "never stored"); an unrecoverable backend
//! error (disk full, permission denied) is reported as `AppError::Config` and
//! surfaces in the UI. These functions never abort the process — the caller
//! decides whether to proceed.

use crate::error::AppError;
use crate::persist;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Filename holding the auth token, relative to the config directory.
const TOKEN_FILENAME: &str = "agent_token";

/// Resolve the auth-token file path from the platform config directory.
///
/// Centralized here so the public API needs no path argument and the
/// path-parameterized helpers can target a temp dir in tests.
fn token_path() -> Result<PathBuf, AppError> {
    Ok(crate::config::GlobalConfig::config_dir()?.join(TOKEN_FILENAME))
}

/// Resolve the token path belonging to one immutable agent-config UUID.
fn agent_token_path(id: &str) -> Result<PathBuf, AppError> {
    agent_token_path_in(&crate::config::GlobalConfig::config_dir()?, id)
}

fn agent_token_path_in(config_dir: &Path, id: &str) -> Result<PathBuf, AppError> {
    validate_agent_id(id)?;
    Ok(config_dir.join(format!("agent_token-{id}")))
}

fn validate_agent_id(id: &str) -> Result<(), AppError> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| AppError::Config(format!("agent config id '{id}' is not a valid UUID")))
}

/// Load the auth token from the local secrets file.
///
/// Returns `Ok(None)` when no file is present (first run, or the user cleared
/// it). Any other backend error is returned as `AppError::Config`.
pub fn load_auth_token() -> Result<Option<String>, AppError> {
    load_from(&token_path()?)
}

/// Delete the auth-token file.
///
/// Idempotent: a missing file is treated as success (there's nothing to
/// delete). Other backend errors are returned.
pub fn delete_auth_token() -> Result<(), AppError> {
    delete_at(&token_path()?)
}

/// Load the token scoped to a named roster entry.
pub fn load_agent_auth_token(id: &str) -> Result<Option<String>, AppError> {
    load_from(&agent_token_path(id)?)
}

/// Store the token scoped to a named roster entry.
pub fn store_agent_auth_token(id: &str, token: &str) -> Result<(), AppError> {
    store_to(&agent_token_path(id)?, token)
}

/// Delete the token scoped to a named roster entry.
pub fn delete_agent_auth_token(id: &str) -> Result<(), AppError> {
    delete_at(&agent_token_path(id)?)
}

/// One-time, idempotent migration from the historical singleton token file to
/// a default roster entry. A token already stored for that UUID wins. The old
/// file is removed only after the destination is durably written.
pub fn migrate_legacy_auth_token_to_agent(id: &str) -> Result<bool, AppError> {
    let legacy_path = token_path()?;
    let target_path = agent_token_path(id)?;
    migrate_legacy_at(id, &legacy_path, &target_path)
}

fn migrate_legacy_at(id: &str, legacy_path: &Path, target_path: &Path) -> Result<bool, AppError> {
    validate_agent_id(id)?;
    let Some(token) = load_from(legacy_path)? else {
        return Ok(false);
    };
    if load_from(target_path)?.is_none() {
        store_to(target_path, &token)?;
    }
    delete_at(legacy_path)?;
    Ok(true)
}

// ── path-parameterized helpers (testable without touching the real config dir) ──

fn load_from(path: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let trimmed = contents.trim_end_matches(['\n', '\r']);
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::Config(format!(
            "failed to read auth token from {}: {e}",
            path.display()
        ))),
    }
}

fn store_to(path: &Path, token: &str) -> Result<(), AppError> {
    // `atomic_write` creates parent dirs, writes to a sibling temp, fsyncs,
    // and renames over the target — a crash mid-write can never truncate an
    // existing token.
    persist::atomic_write(path, token)
        .map_err(|e| AppError::Config(format!("failed to store auth token: {e}")))?;
    restrict_perms(path);
    Ok(())
}

fn delete_at(path: &Path) -> Result<(), AppError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::Config(format!(
            "failed to delete auth token at {}: {e}",
            path.display()
        ))),
    }
}

/// Lock the token file down to owner-only. Best-effort: a failure is logged
/// but not fatal — the file lives under the user's own config directory, so
/// this is defense-in-depth, not the sole access control.
#[cfg(unix)]
fn restrict_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!("failed to set 0600 on {}: {e}", path.display());
    }
}

/// No-op on non-Unix: the token file lives under `%APPDATA%` / `~/Library`,
/// which the OS already scopes to the current user via ACLs. There is no
/// portable `chmod` equivalent.
#[cfg(not(unix))]
fn restrict_perms(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process-unique temp path so parallel `cargo test` threads don't
    /// collide. Tests in the same binary share a PID, and `SystemTime`
    /// resolution alone isn't enough to disambiguate two threads, so we also
    /// draw from a monotonic atomic counter — guaranteed unique per call.
    fn temp_token_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "loopdeck-agent_token-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            n
        ));
        p
    }

    /// Round-trip the token through the local file store. Hermetic — uses a
    /// throwaway temp path, never the real config dir. Runs in the normal
    /// `cargo test` suite (no `#[ignore]`, unlike the old keychain test).
    #[test]
    fn file_backend_roundtrip() {
        let path = temp_token_path();
        // Clean slate in case a prior crashed run left a file behind.
        let _ = delete_at(&path);
        assert_eq!(load_from(&path).unwrap(), None);

        store_to(&path, "sk-roundtrip-token").unwrap();
        assert_eq!(
            load_from(&path).unwrap().as_deref(),
            Some("sk-roundtrip-token")
        );

        // Overwrite.
        store_to(&path, "sk-replacement").unwrap();
        assert_eq!(load_from(&path).unwrap().as_deref(), Some("sk-replacement"));

        // A trailing newline (if a hand-edit ever introduced one) is stripped.
        store_to(&path, "sk-with-newline\n").unwrap();
        assert_eq!(
            load_from(&path).unwrap().as_deref(),
            Some("sk-with-newline")
        );

        // Idempotent delete.
        delete_at(&path).unwrap();
        delete_at(&path).unwrap();
        assert_eq!(load_from(&path).unwrap(), None);

        // An empty file (zero bytes) reads back as None (no token set).
        store_to(&path, "").unwrap();
        assert_eq!(load_from(&path).unwrap(), None);
        let _ = delete_at(&path);
    }

    #[test]
    fn per_agent_paths_are_uuid_scoped_and_isolated() {
        let dir = temp_token_path();
        let first = Uuid::new_v4().to_string();
        let second = Uuid::new_v4().to_string();
        let first_path = agent_token_path_in(&dir, &first).unwrap();
        let second_path = agent_token_path_in(&dir, &second).unwrap();
        assert_ne!(first_path, second_path);

        store_to(&first_path, "first-token").unwrap();
        store_to(&second_path, "second-token").unwrap();
        assert_eq!(
            load_from(&first_path).unwrap().as_deref(),
            Some("first-token")
        );
        assert_eq!(
            load_from(&second_path).unwrap().as_deref(),
            Some("second-token")
        );
        delete_at(&first_path).unwrap();
        assert_eq!(load_from(&first_path).unwrap(), None);
        assert_eq!(
            load_from(&second_path).unwrap().as_deref(),
            Some("second-token")
        );
        let _ = delete_at(&second_path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_secret_migration_is_idempotent_and_never_overwrites_destination() {
        let dir = temp_token_path();
        let id = Uuid::new_v4().to_string();
        let legacy = dir.join("agent_token");
        let destination = agent_token_path_in(&dir, &id).unwrap();
        store_to(&legacy, "legacy-token").unwrap();
        assert!(migrate_legacy_at(&id, &legacy, &destination).unwrap());
        assert_eq!(load_from(&legacy).unwrap(), None);
        assert_eq!(
            load_from(&destination).unwrap().as_deref(),
            Some("legacy-token")
        );
        assert!(!migrate_legacy_at(&id, &legacy, &destination).unwrap());

        store_to(&legacy, "should-not-win").unwrap();
        assert!(migrate_legacy_at(&id, &legacy, &destination).unwrap());
        assert_eq!(
            load_from(&destination).unwrap().as_deref(),
            Some("legacy-token")
        );
        let _ = delete_at(&destination);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_agent_id_cannot_construct_a_secret_path() {
        assert!(agent_token_path_in(Path::new("/tmp"), "../../not-a-uuid").is_err());
    }

    /// Confirm the owner-only permission floor is applied on Unix.
    #[cfg(unix)]
    #[test]
    fn file_backend_sets_0600_on_unix() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_token_path();
        let _ = delete_at(&path);

        store_to(&path, "sk-perms-check").unwrap();
        let mode = std::fs::metadata(&path)
            .expect("token file exists")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "token file must be owner-only (0600), got {:o}",
            mode
        );
        let _ = delete_at(&path);
    }
}
