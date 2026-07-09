//! OS keychain storage for the agent auth token.
//!
//! The auth token used to live in plaintext at `~/.config/loopdeck/config.yaml`.
//! It now lives in the platform-native credential store so it is never written
//! to disk in the clear:
//!
//! - **macOS** — Keychain (`security` framework)
//! - **Windows** — Credential Manager
//! - **Linux** — Secret Service (libsecret via D-Bus)
//!
//! The `keyring` crate abstracts all three behind one API. We store a single
//! secret under a fixed `(service, account)` pair. The rest of the agent config
//! (`base_url`, `model`, `effort`) is non-secret and stays in the YAML file; the
//! token is fetched from the keychain at load time and held only in memory.
//!
//! Keychain failures degrade gracefully: a missing/empty token is reported as
//! `Ok(None)`; an unrecoverable backend error (e.g. headless Linux with no
//! D-Bus) is reported as `AppError::Config` and surfaces in the UI. These
//! functions never abort the process — the caller decides whether to proceed.

use crate::error::AppError;

/// Credential-store identity. Fixed across runs so the token is always found at
/// the same place regardless of which provider/model is configured.
const KEYRING_SERVICE: &str = "loopdeck";
const KEYRING_ACCOUNT: &str = "agent_auth_token";

/// Build the (platform-resolved) keyring `Entry` for the auth token.
fn entry() -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| AppError::Config(format!("keyring entry error: {e}")))
}

/// Load the auth token from the OS keychain.
///
/// Returns `Ok(None)` when no token is stored (first run, or the user cleared
/// it). Any other backend error is returned as `AppError::Config`.
pub fn load_auth_token() -> Result<Option<String>, AppError> {
    match entry()?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Config(format!(
            "failed to read auth token from keychain: {e}"
        ))),
    }
}

/// Store the auth token in the OS keychain, overwriting any existing value.
pub fn store_auth_token(token: &str) -> Result<(), AppError> {
    entry()?
        .set_password(token)
        .map_err(|e| AppError::Config(format!("failed to store auth token in keychain: {e}")))
}

/// Delete the auth token from the OS keychain.
///
/// Idempotent: a missing entry is treated as success (there's nothing to
/// delete). Other backend errors are returned.
pub fn delete_auth_token() -> Result<(), AppError> {
    match entry()?.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Config(format!(
            "failed to delete auth token from keychain: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip the token through the OS keychain.
    ///
    /// Hits the *real* platform credential store, so it's `#[ignore]`d to keep
    /// `cargo test` offline and hermetic (mirrors the `claude_session` live
    /// integration tests). Run explicitly with:
    ///
    /// ```text
    /// cargo test --lib secrets -- --ignored --nocapture
    /// ```
    ///
    /// It cleans up after itself (deletes the entry at the end), so it leaves
    /// no residue in the keychain.
    #[test]
    #[ignore = "writes to the real OS keychain; run with `cargo test -- --ignored`"]
    fn live_keychain_roundtrip() {
        // Ensure a clean slate in case a prior run left an entry.
        let _ = delete_auth_token();
        assert_eq!(load_auth_token().unwrap(), None);

        store_auth_token("sk-roundtrip-token").unwrap();
        assert_eq!(load_auth_token().unwrap().as_deref(), Some("sk-roundtrip-token"));

        // Overwrite.
        store_auth_token("sk-replacement").unwrap();
        assert_eq!(load_auth_token().unwrap().as_deref(), Some("sk-replacement"));

        // Idempotent delete.
        delete_auth_token().unwrap();
        delete_auth_token().unwrap();
        assert_eq!(load_auth_token().unwrap(), None);
    }
}
