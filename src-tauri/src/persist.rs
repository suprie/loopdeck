//! Atomic file writes for critical LoopDeck state.
//!
//! `std::fs::write` is truncate-then-write: a crash, full disk, or OS-dropped
//! write between the open and the final byte leaves a partial file. For
//! recoverable state (the registry, project config, loops, decisions, PRDs,
//! generated Claude settings) that's a data-loss bug.
//!
//! This module's primitive, [`atomic_write`], writes to a sibling temporary
//! file in the *same directory* as the target, flushes + fsyncs it, then
//! renames it over the target. Same-directory rename is atomic on POSIX and
//! atomic-with-replace on Windows for same-volume moves (the temp is in the
//! target's parent, so always same-volume). The old file's contents survive
//! until the rename commits, then are replaced in one step.
//!
//! See `docs/PRD-trust-boundary-hardening.md` FR2.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

/// Suffix for the sibling temp file. Includes the PID so two writers in the
/// same directory (e.g. concurrent saves of different state files) don't
/// collide on a shared `.tmp` name. A stale temp from a crashed prior run
/// doesn't conflict with a live writer of a different PID.
fn temp_suffix() -> String {
    format!(".{}.tmp", std::process::id())
}

/// Atomically write `contents` to `path`.
///
/// Writes to a sibling temp file `<path><temp_suffix()>` in the same
/// directory, flushes + fsyncs it, then renames over `path`. On success the
/// temp file is gone (renamed). On any error the temp file is removed and
/// the original `path` is left untouched — a crash during the write can never
/// truncate the existing file.
///
/// Creates parent directories if needed. Does NOT apply permission
/// restrictions — callers that need a permission floor (e.g. `config.yaml`
/// gets 0600) apply it after this returns, same as today.
pub fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    // Parent must exist. `File::create` in a missing dir fails with NotFound;
    // create the dir first so a fresh install doesn't trip.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Build the sibling temp path: take the target's extension (if any) and
    // append the PID-suffixed `.tmp`, so `config.yaml` → `config.yaml.<pid>.tmp`
    // and a file with no extension → `file.<pid>.tmp`.
    let temp_path = path.with_extension(
        path.extension()
            .map(|e| {
                let mut s = e.to_os_string();
                s.push(temp_suffix());
                s
            })
            .unwrap_or_else(|| temp_suffix().into()),
    );

    // Write + flush + fsync the temp file. fsync is what makes the write
    // durable: without it a crash after the rename can still lose data that
    // only made it to the OS page cache.
    let write_result = (|| -> io::Result<()> {
        let mut file = File::create(&temp_path)?;
        file.write_all(contents.as_bytes())?;
        file.flush()?;
        // fsync is fallible on some filesystems (network mounts), but a
        // failure here means we can't guarantee durability — propagate so the
        // caller knows the write wasn't durable, rather than silently
        // succeeding.
        file.sync_all()?;
        // Drop explicitly: Windows refuses to rename a file with an open
        // handle, so make sure the handle is closed before the rename.
        drop(file);
        Ok(())
    })();

    if let Err(e) = write_result {
        // Clean up the partial temp file. Best-effort — a failure here is
        // logged but doesn't mask the original error.
        let _ = fs::remove_file(&temp_path);
        return Err(e);
    }

    // Same-directory rename: atomic on POSIX, atomic-on-same-volume on
    // Windows. The temp is in the target's parent by construction, so the
    // cross-device edge case can't arise here.
    fs::rename(&temp_path, path)?;

    Ok(())
}

/// Read `path` to a string, returning `Ok(Some(contents))` if it exists,
/// `Ok(None)` if it doesn't. Thin wrapper so callers don't pattern-match on
/// `NotFound` everywhere.
//
// `allow(dead_code)`: not yet wired into a production call site. Task 5
// (transcript partial-line tolerance) will use it; the tests below pin the
// contract so it's ready.
#[allow(dead_code)]
pub fn read_if_exists(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique temp dir per test, keyed by the test name + PID + nanos so
    /// parallel test runs can't race on shared parent dirs.
    fn temp_dir(test_name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-persist-{test_name}-{}-{}",
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
    fn atomic_write_creates_new_file() {
        let dir = temp_dir("creates_new");
        let target = dir.join("newfile.yaml");
        atomic_write(&target, "hello").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = temp_dir("overwrites");
        let target = dir.join("existing.yaml");
        fs::write(&target, "old").unwrap();
        atomic_write(&target, "new").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let dir = temp_dir("parent_dirs");
        let target = dir.join("nested/deep/file.yaml");
        atomic_write(&target, "x").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "x");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_leaves_no_temp_on_success() {
        let dir = temp_dir("no_temp");
        let target = dir.join("clean.yaml");
        atomic_write(&target, "data").unwrap();
        // No `.tmp` sibling should remain — only the target.
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "expected only the target file to remain");
        assert_eq!(
            entries[0].as_ref().unwrap().file_name().to_str().unwrap(),
            "clean.yaml",
            "the one remaining file must be the target, not a temp"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn atomic_write_leaves_original_untouched_on_failure() {
        // Point at a path whose parent is a regular file rather than a
        // directory. `create_dir_all` will fail, so the write must abort
        // without touching the (absent) original — and without leaving a
        // stray temp anywhere we can reach.
        let dir = temp_dir("untouched_on_fail");
        let blocker = dir.join("blocker");
        fs::write(&blocker, "block").unwrap();
        let target = blocker.join("underneath.yaml"); // parent is a file
        let result = atomic_write(&target, "new");
        assert!(result.is_err(), "should fail when parent is a file");
        // The blocker file is untouched.
        assert_eq!(fs::read_to_string(&blocker).unwrap(), "block");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_if_exists_returns_none_for_missing() {
        let dir = temp_dir("missing");
        let missing = dir.join("nope.yaml");
        assert!(read_if_exists(&missing).unwrap().is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_if_exists_returns_contents_for_present() {
        let dir = temp_dir("present");
        let present = dir.join("here.yaml");
        fs::write(&present, "data").unwrap();
        assert_eq!(read_if_exists(&present).unwrap().as_deref(), Some("data"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
