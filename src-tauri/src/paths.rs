//! Central project-boundary helpers (PRD FR3).
//!
//! Every project-scoped IPC command funnels its path argument through these
//! helpers so that:
//!
//! 1. The path resolves to a real, **registered** project root
//!    ([`resolve_registered_root`]).
//! 2. Any relative sub-path beneath that root stays beneath it — `..`
//!    traversal, absolute-path replacement, and symlink escape are all
//!    rejected ([`resolve_within`]).
//!
//! Before this module, each command reimplemented containment ad hoc (or not
//! at all): most did `PathBuf::from(&path)` + `if path.exists()` and trusted
//! the result. Now correctness does not depend on each new command getting the
//! check right — the helpers are the chokepoint.
//!
//! See `docs/PRD-trust-boundary-hardening.md` FR3, and the loops.md history
//! entry for the same date.

use crate::config::GlobalConfig;
use crate::error::AppError;
use std::path::{Component, Path, PathBuf};

// ── Registered-root resolution ─────────────────────────────────────────────

/// Canonicalize a raw project-path argument to an absolute, normalized form
/// and confirm it is an existing directory.
///
/// This is the filesystem half of the boundary check; [`resolve_registered_root`]
/// layers the registry check on top. Raw IPC paths are untrusted input, so we
/// normalize before any comparison and refuse anything that isn't a real
/// directory we can walk into.
pub fn canonical_root(raw: &str) -> Result<PathBuf, AppError> {
    let resolved = std::fs::canonicalize(raw)
        .map_err(|e| AppError::InvalidPath(format!("project path does not resolve: {e}")))?;
    if !resolved.is_dir() {
        return Err(AppError::InvalidPath(format!(
            "project path is not a directory: {raw}"
        )));
    }
    Ok(resolved)
}

/// Resolve `raw` to a canonical, **registered** project root.
///
/// Borrows `config` rather than taking the `AppState` lock, so the caller
/// controls lock lifetime — this matters because most callers must release the
/// config lock before doing blocking I/O (git probes, walks) on the returned
/// root.
///
/// Returns [`AppError::ProjectNotFound`] when the canonical path isn't in the
/// registry. That is the structured "missing/moved" state the PRD calls for:
/// the caller surfaces it as a recoverable error (re-import / re-link) rather
/// than silently operating on an unregistered directory. Reusing the existing
/// `ProjectNotFound` kind keeps the frontend's error surface unchanged.
pub fn resolve_registered_root(config: &GlobalConfig, raw: &str) -> Result<PathBuf, AppError> {
    let canonical = canonical_root(raw)?;
    if config.find_by_path(&canonical).is_some() {
        Ok(canonical)
    } else {
        Err(AppError::ProjectNotFound(format!(
            "no project registered at {}; import it first",
            canonical.display()
        )))
    }
}

// ── Contained relative-path resolution ─────────────────────────────────────

/// Resolve a relative path beneath `root`, rejecting traversal and symlink
/// escape.
///
/// `root` MUST be canonical (the result of [`canonical_root`] /
/// [`resolve_registered_root`]); the `starts_with` containment check below
/// relies on both sides being normalized.
///
/// `must_exist` selects the validation strategy:
/// - `true` (reads / listings): canonicalize the target. A symlink pointing
///   outside `root` resolves out and is rejected by the `starts_with` check.
/// - `false` (writes): the target may not exist yet, so the longest existing
///   ancestor is canonicalized ([`canonicalize_prefix`]) and the remaining
///   components re-appended — a symlink in the existing prefix still resolves
///   out and is rejected.
///
/// Rejected in every case, before touching the filesystem where possible:
/// - an absolute `rel` (`/etc/passwd` would *replace* `root` on `join`), and
/// - any `..` component (traversal).
pub fn resolve_within(root: &Path, rel: &str, must_exist: bool) -> Result<PathBuf, AppError> {
    // Lexical guard before any filesystem work: reject `..` and absolute paths.
    // `Path::join` with an absolute argument replaces the base entirely — the
    // classic containment bug — so `RootDir` is rejected here too.
    for component in Path::new(rel).components() {
        match component {
            Component::ParentDir => {
                return Err(AppError::InvalidPath(
                    "relative path contains `..` — traversal rejected".into(),
                ));
            }
            Component::RootDir => {
                return Err(AppError::InvalidPath(
                    "relative path is absolute — escape rejected".into(),
                ));
            }
            Component::Prefix(_) => {
                // A Windows drive prefix (e.g. `C:\`) is also an absolute root.
                return Err(AppError::InvalidPath(
                    "relative path carries a drive prefix — escape rejected".into(),
                ));
            }
            _ => {}
        }
    }

    let target = root.join(rel);
    let resolved = if must_exist {
        target.canonicalize().map_err(|e| {
            AppError::InvalidPath(format!("path does not resolve beneath root: {e}"))
        })?
    } else {
        canonicalize_prefix(&target)?
    };

    if !resolved.starts_with(root) {
        return Err(AppError::InvalidPath(
            "path is outside the project root".into(),
        ));
    }
    Ok(resolved)
}

/// Canonicalize the longest existing prefix of `path` and re-append the
/// remaining (non-existent) components.
///
/// Used for write-path validation where the target file or its parent
/// directory may not exist yet — [`Path::canonicalize`] fails on a non-existent
/// path, so we walk up to the first existing ancestor, canonicalize that
/// (resolving any symlinks in the prefix), then push the remaining components
/// back on. A symlinked prefix that escapes `root` is caught by the caller's
/// `starts_with` check, since the canonicalized prefix reflects the real
/// destination.
pub(crate) fn canonicalize_prefix(path: &Path) -> Result<PathBuf, AppError> {
    // Collect components from the leaf upward until we find an existing path.
    let mut to_append: Vec<std::ffi::OsString> = Vec::new();
    let mut current = path.to_path_buf();

    loop {
        match current.canonicalize() {
            Ok(canon) => {
                let mut result = canon;
                for comp in to_append.into_iter().rev() {
                    result.push(comp);
                }
                return Ok(result);
            }
            Err(_) => {
                // Strip the last component and try again.
                let parent = current.parent();
                let filename = current.file_name();
                match (parent, filename) {
                    (Some(p), Some(f)) => {
                        to_append.push(f.to_os_string());
                        current = p.to_path_buf();
                    }
                    _ => {
                        return Err(AppError::InvalidPath(format!(
                            "cannot canonicalize any prefix of: {}",
                            path.display()
                        )));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalConfig, ProjectEntry};
    use chrono::Utc;

    /// Unique temp dir per test, keyed by name + PID + nanos so parallel tests
    /// can't race on shared parents. Returns the **canonical** path, because
    /// the helpers under test require a canonical root — on macOS `temp_dir()`
    /// returns `/var/folders/...` which `canonicalize` resolves to
    /// `/private/var/...`, so a non-canonical fixture would falsely look
    /// "outside the root".
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-paths-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn registered_config(root: &Path) -> GlobalConfig {
        let mut config = GlobalConfig::default();
        config.projects.push(ProjectEntry {
            path: root.to_path_buf(),
            name: "test".into(),
            created_at: Utc::now(),
            ..Default::default()
        });
        config
    }

    // ── canonical_root ─────────────────────────────────────────────────

    #[test]
    fn canonical_root_resolves_existing_dir() {
        let dir = scratch("canon_ok");
        // A subpath with a trailing `.` resolves to the canonical dir.
        let child = dir.join("child");
        std::fs::create_dir_all(&child).unwrap();
        let resolved = canonical_root(child.to_string_lossy().to_string().as_str()).unwrap();
        assert_eq!(resolved, child);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn canonical_root_rejects_missing() {
        let dir = scratch("canon_missing");
        let missing = dir.join("nope");
        let err = canonical_root(missing.to_string_lossy().to_string().as_str());
        assert!(err.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn canonical_root_rejects_file() {
        let dir = scratch("canon_file");
        let file = dir.join("a-file");
        std::fs::write(&file, "x").unwrap();
        let err = canonical_root(file.to_string_lossy().to_string().as_str());
        assert!(err.is_err(), "a file is not a valid project root");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── resolve_registered_root ────────────────────────────────────────

    #[test]
    fn resolve_registered_root_accepts_registered() {
        let dir = scratch("reg_ok");
        let config = registered_config(&dir);
        let resolved =
            resolve_registered_root(&config, dir.to_string_lossy().to_string().as_str()).unwrap();
        assert_eq!(resolved, dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_registered_root_rejects_unregistered() {
        let dir = scratch("reg_no");
        // An empty config — the dir exists on disk but is NOT registered.
        let config = GlobalConfig::default();
        let err = resolve_registered_root(&config, dir.to_string_lossy().to_string().as_str());
        match err {
            Err(AppError::ProjectNotFound(_)) => {}
            other => panic!("expected ProjectNotFound, got {other:?}"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── resolve_within — must_exist (reads / listings) ─────────────────

    #[test]
    fn resolve_within_read_accepts_nested_existing() {
        let dir = scratch("rw_read_nested");
        let nested = dir.join("src").join("sub");
        std::fs::create_dir_all(&nested).unwrap();

        let resolved = resolve_within(&dir, "src/sub", true).unwrap();
        assert_eq!(resolved, nested);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_within_read_rejects_traversal() {
        let dir = scratch("rw_read_traversal");
        let err = resolve_within(&dir, "../../etc/passwd", true);
        assert!(matches!(err, Err(AppError::InvalidPath(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_within_read_rejects_absolute() {
        let dir = scratch("rw_read_abs");
        // An absolute rel would replace the root on join — must be rejected
        // before any filesystem access.
        let err = resolve_within(&dir, "/etc/passwd", true);
        assert!(matches!(err, Err(AppError::InvalidPath(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_within_read_rejects_symlink_escape() {
        // A symlink inside the root that points outside it must be rejected:
        // canonicalizing the target resolves it out from under the root.
        let dir = scratch("rw_read_symlink");
        let outside = scratch("rw_read_symlink_outside");
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, dir.join("escape")).unwrap();
            let err = resolve_within(&dir, "escape/secret.txt", true);
            assert!(
                matches!(err, Err(AppError::InvalidPath(_))),
                "symlink escaping the root must be rejected"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn resolve_within_empty_rel_returns_root() {
        // Listing the root itself (empty subdir) must resolve cleanly.
        let dir = scratch("rw_empty");
        let resolved = resolve_within(&dir, "", true).unwrap();
        assert_eq!(resolved, dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    // ── resolve_within — !must_exist (writes) ──────────────────────────

    #[test]
    fn resolve_within_write_accepts_new_nested_file() {
        // A brand-new file under an existing parent: canonicalize the parent,
        // re-append the filename.
        let dir = scratch("rw_write_new");
        std::fs::create_dir_all(dir.join("docs")).unwrap();

        let resolved = resolve_within(&dir, "docs/new.md", false).unwrap();
        assert_eq!(resolved, dir.join("docs").join("new.md"));
        // And a write actually lands there.
        std::fs::write(&resolved, "x").unwrap();
        assert!(dir.join("docs").join("new.md").exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_within_write_rejects_traversal() {
        let dir = scratch("rw_write_traversal");
        let err = resolve_within(&dir, "../../etc/evil.md", false);
        assert!(matches!(err, Err(AppError::InvalidPath(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_within_write_rejects_absolute() {
        let dir = scratch("rw_write_abs");
        let err = resolve_within(&dir, "/etc/evil.md", false);
        assert!(matches!(err, Err(AppError::InvalidPath(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_within_write_rejects_symlinked_prefix_escape() {
        // Write path: a symlinked directory inside the root pointing outside.
        // canonicalize_prefix resolves the symlinked ancestor to its real
        // destination (outside the root), so starts_with rejects it.
        let dir = scratch("rw_write_symlink");
        let outside = scratch("rw_write_symlink_outside");
        std::fs::create_dir_all(&outside).unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, dir.join("escape")).unwrap();
            let err = resolve_within(&dir, "escape/planted.md", false);
            assert!(
                matches!(err, Err(AppError::InvalidPath(_))),
                "write via an escaping symlinked dir must be rejected"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&outside).unwrap();
    }

    // ── canonicalize_prefix ────────────────────────────────────────────

    #[test]
    fn canonicalize_prefix_handles_missing_leaf_and_parent() {
        // Neither the file nor its parent exists; the grandparent does.
        let dir = scratch("cprefix");
        std::fs::create_dir_all(dir.join("grandparent")).unwrap();
        let target = dir.join("grandparent").join("parent").join("file.md");

        let resolved = canonicalize_prefix(&target).unwrap();
        assert_eq!(
            resolved, target,
            "non-existent tail components are preserved"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
