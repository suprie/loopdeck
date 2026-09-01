//! Absolute, vetted resolution of subprocess binaries (`claude`, `codex`, `git`).
//!
//! `Command::new("git")` / `Command::new("claude")` hand a bare name to the
//! OS, which searches `$PATH` at spawn time. That has two weaknesses this
//! module closes:
//!
//! 1. **cwd hijack via a relative PATH entry.** An empty or `.` component in
//!    `$PATH` means "the current directory". Selasar spawns `claude` with
//!    `current_dir` set to a user-selected project and `git` against a scanned
//!    repo — so a project shipping a `claude`/`git` script could otherwise run
//!    that script, with the agent auth token in the child environment for the
//!    `claude` case. We refuse to search any non-absolute PATH component,
//!    closing that vector entirely.
//!
//! 2. **unvetted match.** The OS runs the first match without confirming it is
//!    actually executable. We verify the candidate is a regular file with an
//!    executable bit (Unix) before accepting it.
//!
//! Resolved paths are pinned for the process lifetime via a [`OnceLock`], so a
//! later mutation of `$PATH` (by a sibling/child process) cannot redirect
//! subsequent spawns. A resolution that fails keeps retrying until the first
//! success — installing the binary mid-session and retrying picks it up.
//!
//! What this *cannot* prevent: if `$PATH` itself is compromised, a legitimate
//! earlier entry wins by design. The wins here are what a non-privileged app
//! can achieve — kill the cwd vector, vet executability, and surface the exact
//! resolved path at spawn.
//!
//! Caveat: a GUI-launched app inherits a minimal `$PATH` (no Homebrew / npm
//! global dirs), so a binary the user installed via a shell-only PATH entry may
//! not resolve here — same as the bare-name spawn it replaces. Discovering
//! common install dirs is a separate follow-up, out of scope for the hijack fix.

use crate::error::AppError;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Resolve `name` (a bare executable name) against the process `$PATH`,
/// returning the first absolute, executable match. See the module docs for the
/// hijack defenses applied.
pub(crate) fn resolve_command(name: &str) -> Result<PathBuf, AppError> {
    let path_var = std::env::var_os("PATH")
        .ok_or_else(|| AppError::Config(format!("cannot resolve {name:?}: $PATH is not set")))?;
    resolve_in(name, &path_var)
}

/// PATH-scoped core of [`resolve_command`], factored out so the hijack defenses
/// can be unit-tested without mutating the process-global `$PATH`.
fn resolve_in(name: &str, path_var: &OsStr) -> Result<PathBuf, AppError> {
    for dir in std::env::split_paths(path_var) {
        // Skip any non-absolute component: an empty string or a relative path
        // (incl. `.`) resolves against the process cwd — a user-selected
        // project for `claude`, a scanned repo for `git`. That is the cwd-hijack
        // vector this module exists to close.
        if !dir.is_absolute() {
            continue;
        }
        // Skip npm-injected `node_modules/.bin` dirs: `npm run`/`npx` prepend
        // the nearest package's `.bin` onto `$PATH` for the lifetime of that
        // script, so launching from a monorepo root can shadow the real
        // system binary with whatever version that one package happens to
        // depend on (see issue #45 — `codex` resolved to a stale copy inside
        // `node_modules/@openai/codex` instead of `/usr/local/bin/codex`).
        if dir.ends_with("node_modules/.bin") {
            continue;
        }
        for candidate in candidate_paths(&dir, name) {
            if let Some(resolved) = vet(&candidate) {
                return Ok(resolved);
            }
        }
    }
    Err(AppError::Config(format!(
        "cannot resolve {name:?}: not found in any absolute $PATH directory"
    )))
}

/// The candidate path(s) to check for `name` within `dir`. On Windows a bare
/// name also probes the standard executable extensions (mirroring PATHEXT);
/// elsewhere a single direct path suffices.
fn candidate_paths(dir: &Path, name: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        // Already has an extension (e.g. `git.exe`) → try it directly.
        if name.rsplit('.').count() > 1 {
            return vec![dir.join(name)];
        }
        ["exe", "bat", "cmd", "com"]
            .iter()
            .map(|ext| dir.join(format!("{name}.{ext}")))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![dir.join(name)]
    }
}

/// Vet a single candidate: return it as `Some` if it is an existing executable
/// regular file, else `None` so the search continues. Non-`NotFound` stat
/// errors are logged at debug and treated as "not a match" so one unreadable
/// PATH entry can't abort the whole search.
fn vet(candidate: &Path) -> Option<PathBuf> {
    // `metadata` (not `symlink_metadata`) follows symlinks — we vet what
    // actually runs, the link target.
    let metadata = match std::fs::metadata(candidate) {
        Ok(m) => m,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::debug!(
                    target: "loopdeck::binary",
                    "skipping non-resolvable PATH candidate {}: {e}",
                    candidate.display()
                );
            }
            return None;
        }
    };
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Any execute bit — user, group, or other — suffices.
        if metadata.mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(candidate.to_path_buf())
}

/// Cached resolution of the `git` binary — resolved once per process lifetime.
/// Returns the pinned absolute path on success; re-resolves on each call until
/// the first success (so a mid-session install is picked up).
///
/// Uses the stable `OnceLock::get`/`set` pair (not `get_or_try_init`, which is
/// still unstable). The loop is the fallible-init dance: a failed resolution
/// returns `Err` *without* populating the cell, so the next call tries again;
/// once `set` succeeds (or a concurrent caller beat us to it) `get()` is `Some`
/// and the loop exits after at most two iterations.
pub(crate) fn git() -> Result<&'static Path, AppError> {
    static GIT: OnceLock<PathBuf> = OnceLock::new();
    loop {
        if let Some(p) = GIT.get() {
            return Ok(p.as_path());
        }
        let resolved = resolve_command("git")?;
        // Lost-race tolerant: `set` is a no-op if another caller initialized the
        // cell first. Either way it's populated now → the next `get()` hits.
        let _ = GIT.set(resolved);
    }
}

/// Cached resolution of the `claude` binary — see [`git`] for caching semantics.
pub(crate) fn claude() -> Result<&'static Path, AppError> {
    static CLAUDE: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = env_override("LOOPDECK_CLAUDE_BIN") {
        return Ok(path.leak());
    }
    loop {
        if let Some(p) = CLAUDE.get() {
            return Ok(p.as_path());
        }
        let resolved = resolve_command("claude")?;
        let _ = CLAUDE.set(resolved);
    }
}

/// Cached resolution of the `codex` binary — see [`git`] for caching semantics.
pub(crate) fn codex() -> Result<&'static Path, AppError> {
    static CODEX: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = env_override("LOOPDECK_CODEX_BIN") {
        return Ok(path.leak());
    }
    loop {
        if let Some(p) = CODEX.get() {
            return Ok(p.as_path());
        }
        let resolved = resolve_command("codex")?;
        let _ = CODEX.set(resolved);
    }
}

/// Absolute-path binary override from the environment (`LOOPDECK_CLAUDE_BIN`
/// / `LOOPDECK_CODEX_BIN`). Exists so tests can point the real spawn code at
/// fixture binaries that capture their own argv. Only an absolute, existing,
/// executable file qualifies — anything else falls back to the vetted PATH
/// search. Checked before the cache so the override is re-read per spawn.
fn env_override(var: &str) -> Option<PathBuf> {
    let value = std::env::var_os(var)?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return None;
    }
    vet(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `$PATH`-style value from `&str` components. Components must not
    /// contain the platform path separator (`:` / `;`).
    fn path_var(paths: &[&str]) -> std::ffi::OsString {
        std::env::join_paths(paths.iter().map(PathBuf::from))
            .expect("test paths must not contain the path separator")
    }

    /// `git` is guaranteed present wherever these tests run (they spawn git),
    /// so resolving it against the real env must yield an absolute file. Runs
    /// best-effort so a stripped-down CI image without git doesn't hard-fail.
    #[test]
    fn resolves_real_git_from_env_path() {
        match resolve_command("git") {
            Ok(resolved) => {
                assert!(resolved.is_absolute(), "resolved path must be absolute");
                assert!(resolved.is_file(), "resolved binary must be a real file");
            }
            Err(_) => {
                eprintln!("git not on PATH — skipping real-env resolution assertion");
            }
        }
    }

    /// The core hijack defense: even a known-installed binary must NOT resolve
    /// when only relative PATH entries (incl. `.` and empty) are present.
    #[test]
    fn ignores_relative_only_path_entries() {
        let pv = path_var(&[".", "", "some/relative/dir"]);
        assert!(
            resolve_in("git", &pv).is_err(),
            "git must not resolve from a relative-only PATH"
        );
    }

    /// Relative entries are skipped but resolution still succeeds from a later
    /// absolute dir. Deterministic: plants an exec file in a temp absolute dir.
    #[cfg(unix)]
    #[test]
    fn skips_relative_finds_absolute_in_temp() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ld-resolve-mix-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ld-mix-bin");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();

        let dir_s = dir.to_str().expect("temp dir is utf-8");
        // Relative junk first, then the absolute temp dir holding the binary.
        let pv = path_var(&[".", "", "relative", dir_s]);
        let resolved = resolve_in("ld-mix-bin", &pv)
            .expect("must resolve from the absolute temp dir, skipping relative entries");
        assert_eq!(resolved, bin);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn errors_when_not_found() {
        let pv = path_var(&["/usr/bin", "/bin"]);
        assert!(resolve_in("loopdeck-nonexistent-binary-zzz", &pv).is_err());
    }

    /// A file that exists but lacks the execute bit must be skipped.
    #[cfg(unix)]
    #[test]
    fn rejects_non_executable_file() {
        let dir = std::env::temp_dir().join(format!("ld-resolve-noexec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ld-fake-bin");
        std::fs::write(&bin, "#!/bin/sh\necho nope\n").unwrap();
        // Default perms (0644) — no execute bit.

        let dir_s = dir.to_str().expect("temp dir is utf-8");
        let pv = path_var(&[dir_s]);
        assert!(
            resolve_in("ld-fake-bin", &pv).is_err(),
            "a file without the execute bit must not resolve"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A regular executable file in an absolute dir resolves to itself.
    #[cfg(unix)]
    #[test]
    fn accepts_executable_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ld-resolve-exec-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("ld-ok-bin");
        std::fs::write(&bin, "#!/bin/sh\necho ok\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();

        let dir_s = dir.to_str().expect("temp dir is utf-8");
        let pv = path_var(&[dir_s]);
        let resolved = resolve_in("ld-ok-bin", &pv)
            .expect("an executable file in an absolute dir must resolve");
        assert_eq!(resolved, bin);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A `node_modules/.bin` PATH entry is skipped even when it holds a valid
    /// executable, so an npm-injected shim can't shadow the real binary
    /// (issue #45).
    #[cfg(unix)]
    #[test]
    fn skips_node_modules_bin_dir() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("ld-resolve-nmbin-{}", uuid::Uuid::new_v4()));
        let nm_bin = root.join("node_modules/.bin");
        std::fs::create_dir_all(&nm_bin).unwrap();
        let shim = nm_bin.join("ld-nm-bin");
        std::fs::write(&shim, "#!/bin/sh\n").unwrap();
        let mut perms = std::fs::metadata(&shim).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&shim, perms).unwrap();

        let nm_bin_s = nm_bin.to_str().expect("temp dir is utf-8");
        let pv = path_var(&[nm_bin_s]);
        assert!(
            resolve_in("ld-nm-bin", &pv).is_err(),
            "node_modules/.bin must be skipped even with a valid executable"
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    /// An absolute-but-nonexistent dir is skipped silently; resolution
    /// continues to later dirs.
    #[test]
    fn skips_nonexistent_absolute_dir() {
        // Deterministic via the real git binary on the host (best-effort).
        let pv = path_var(&["/definitely/not/a/real/path/loopdeck", "/usr/bin", "/bin"]);
        match resolve_in("git", &pv) {
            Ok(resolved) => assert!(resolved.is_absolute()),
            Err(_) => {
                eprintln!("git not in /usr/bin or /bin on this host — skipping");
            }
        }
    }
}
