use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::UNIX_EPOCH;

/// A linked worktree created for an unattended run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Worktree {
    pub path: std::path::PathBuf,
    pub branch: String,
}

/// Whether optional local commit evidence can be proved from the current
/// repository without network access.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommitReachability {
    Missing,
    Reachable,
    Unreachable,
}

/// Build a `git` command pinned to the vetted, resolved binary and aimed at
/// `repo_path` via `-C`. Returns `None` when the `git` binary can't be resolved
/// (see `binary::git`) — callers treat that as "no git info", matching the
/// prior behavior of a failed spawn.
///
/// Resolving here (instead of `Command::new("git")`) defeats the cwd-hijack
/// vector: a bare name would let a `.`/empty `$PATH` entry resolve against
/// `repo_path` (a scanned, untrusted directory), running a planted `git`
/// script. The resolved path is also pinned for the process lifetime.
fn git_command(repo_path: &Path) -> Option<Command> {
    let git = crate::binary::git().ok()?;
    let mut cmd = Command::new(git);
    cmd.args(["-C"]).arg(repo_path);
    Some(cmd)
}

/// Best-effort `git init` for a fresh project directory. Never fails the
/// caller: a missing git binary or failed spawn returns `false`, and a
/// project without git simply degrades (epics stay authorable; promote-with-
/// evidence and PR flows need a real repo).
pub fn git_init(repo_path: &Path) -> bool {
    let Some(mut cmd) = git_command(repo_path) else {
        return false;
    };
    cmd.arg("init")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a new branch-backed linked worktree from `repo_path`.
///
/// The caller chooses a path outside the main worktree and a run-scoped branch
/// name. Git refuses an already checked-out branch, which is the desired
/// protection against two runs sharing one mutable tree.
pub fn worktree_add(repo_path: &Path, path: &Path, branch: &str) -> Result<Worktree, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("worktree path {} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("failed to create worktree parent {}: {e}", parent.display()))?;
    let mut command = git_command(repo_path)
        .ok_or_else(|| "git is unavailable; cannot create isolated run worktree".to_string())?;
    let output = command
        .args(["worktree", "add", "-b", branch])
        .arg(path)
        .arg("HEAD")
        .output()
        .map_err(|e| format!("failed to spawn git worktree add: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git worktree add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    // Git canonicalizes linked-worktree paths in `worktree list` (notably
    // `/var` → `/private/var` on macOS). Persist that same canonical form so a
    // restarted unattended run can match its saved path to Git's inventory.
    let canonical_path = std::fs::canonicalize(path)
        .map_err(|e| format!("failed to canonicalize worktree {}: {e}", path.display()))?;
    Ok(Worktree {
        path: canonical_path,
        branch: branch.to_string(),
    })
}

/// Remove a linked worktree after a successful unattended draft-PR run.
///
/// Forced: a successful run leaves the worktree dirty with incidental
/// post-push writes (e.g. `.loopdeck/loops.md` loop-state updates), and git
/// refuses to remove a dirty worktree without `--force`. The branch was
/// already pushed for the draft PR, so the worktree holds no work worth
/// preserving; anything still on disk is a run artifact.
pub fn worktree_remove(repo_path: &Path, path: &Path) -> Result<(), String> {
    let mut command = git_command(repo_path)
        .ok_or_else(|| "git is unavailable; cannot prune isolated run worktree".to_string())?;
    let output = command
        .args(["worktree", "remove", "--force"])
        .arg(path)
        .output()
        .map_err(|e| format!("failed to spawn git worktree remove: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git worktree remove failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

/// Return the paths of all worktrees Git currently knows about for a repo.
pub fn worktree_list(repo_path: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut command = git_command(repo_path)
        .ok_or_else(|| "git is unavailable; cannot list worktrees".to_string())?;
    let output = command
        .args(["worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("failed to spawn git worktree list: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git worktree list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(std::path::PathBuf::from)
        .collect())
}

/// Classify recorded commit evidence. A commit is reachable only when it
/// resolves to a commit object and is an ancestor of the current `HEAD`.
pub fn commit_reachability(repo_path: &Path, commit: Option<&str>) -> CommitReachability {
    let Some(commit) = commit.map(str::trim).filter(|value| !value.is_empty()) else {
        return CommitReachability::Missing;
    };
    if !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return CommitReachability::Unreachable;
    }

    let Some(mut resolve) = git_command(repo_path) else {
        return CommitReachability::Unreachable;
    };
    let revision = format!("{commit}^{{commit}}");
    let Ok(resolved) = resolve
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(&revision)
        .output()
    else {
        return CommitReachability::Unreachable;
    };
    if !resolved.status.success() {
        return CommitReachability::Unreachable;
    }

    let canonical = String::from_utf8_lossy(&resolved.stdout).trim().to_string();
    let Some(mut ancestor) = git_command(repo_path) else {
        return CommitReachability::Unreachable;
    };
    match ancestor
        .args(["merge-base", "--is-ancestor"])
        .arg(&canonical)
        .arg("HEAD")
        .status()
    {
        Ok(status) if status.success() => CommitReachability::Reachable,
        _ => CommitReachability::Unreachable,
    }
}

/// Resolve user-supplied evidence to the canonical commit SHA, rejecting
/// missing or unreachable objects before they enter execution state.
pub fn verified_commit(repo_path: &Path, commit: &str) -> Result<String, String> {
    if commit_reachability(repo_path, Some(commit)) != CommitReachability::Reachable {
        return Err(format!(
            "commit evidence \"{}\" is missing or not reachable from HEAD",
            commit.trim()
        ));
    }
    let mut resolve = git_command(repo_path)
        .ok_or_else(|| "git is unavailable; commit evidence cannot be verified".to_string())?;
    let revision = format!("{}^{{commit}}", commit.trim());
    let output = resolve
        .args(["rev-parse", "--verify"])
        .arg(&revision)
        .output()
        .map_err(|e| format!("failed to verify commit evidence: {e}"))?;
    if !output.status.success() {
        return Err(format!("commit evidence \"{}\" is invalid", commit.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Git and filesystem freshness info for a project directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    /// ISO 8601 timestamp of the last git commit, or None if no commits / no git.
    pub last_commit_date: Option<String>,
    pub last_commit_message: Option<String>,
    /// Whether the working tree has uncommitted changes.
    pub is_dirty: bool,
    /// ISO 8601 timestamp of the most recently modified tracked file.
    /// Fallback: newest filesystem modification time (skipping ignorable dirs).
    pub last_modified: Option<String>,
    /// Uncommitted change summary: number of changed files, lines added, lines
    /// deleted. All zero when the working tree is clean or there's no git.
    #[serde(default)]
    pub uncommitted: UncommittedStats,
}

/// Aggregate diff stats for an uncommitted working tree.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UncommittedStats {
    /// Number of files with uncommitted changes (staged + unstaged + untracked).
    pub files: u32,
    /// Total inserted lines across the unstaged diff. Untracked files report 0.
    pub added: u32,
    /// Total deleted lines across the unstaged diff.
    pub deleted: u32,
}

/// Check git info and filesystem freshness for a directory.
pub fn check_git_info(path: &Path) -> GitInfo {
    let has_git = path.join(".git").exists();

    let (last_commit_date, last_commit_message, is_dirty, uncommitted) = if has_git {
        let last_commit_date = last_commit_date(path);
        let dirty = has_uncommitted_changes(path);
        let last_commit_message = last_commit_message(path);
        let uncommitted = if dirty {
            uncommitted_stats(path)
        } else {
            UncommittedStats::default()
        };
        (last_commit_date, last_commit_message, dirty, uncommitted)
    } else {
        (None, None, false, UncommittedStats::default())
    };

    let last_modified = last_modified_time(path);
    GitInfo {
        last_commit_date,
        last_commit_message,
        is_dirty,
        last_modified,
        uncommitted,
    }
}
/// Get the commit message from last git commit.
fn last_commit_message(repo_path: &Path) -> Option<String> {
    let mut cmd = git_command(repo_path)?;
    let output = cmd.args(["log", "-1", "--format=%s"]).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let date = stdout.trim().to_string();

    if date.is_empty() {
        None
    } else {
        Some(date)
    }
}

/// Get the ISO 8601 date of the last git commit.
fn last_commit_date(repo_path: &Path) -> Option<String> {
    let mut cmd = git_command(repo_path)?;
    let output = cmd.args(["log", "-1", "--format=%cI"]).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let date = stdout.trim().to_string();

    if date.is_empty() {
        None
    } else {
        Some(date)
    }
}

/// Check if the working tree has uncommitted changes.
fn has_uncommitted_changes(repo_path: &Path) -> bool {
    let Some(mut cmd) = git_command(repo_path) else {
        return false;
    };
    let output = cmd.args(["status", "--porcelain"]).output();

    match output {
        Ok(out) => !out.stdout.is_empty(),
        Err(_) => false,
    }
}

/// True when a linked worktree contains no staged, unstaged, or untracked
/// changes. Cleanup callers use this as a deliberately conservative gate:
/// inability to inspect the tree is treated as dirty and therefore retained.
pub fn worktree_is_pristine(repo_path: &Path) -> bool {
    let Some(mut cmd) = git_command(repo_path) else {
        return false;
    };
    match cmd.args(["status", "--porcelain"]).output() {
        Ok(output) if output.status.success() => output.stdout.is_empty(),
        _ => false,
    }
}

/// Aggregate uncommitted change stats for a dirty working tree.
///
/// `files` counts every entry in `git status --porcelain` (staged + unstaged +
/// untracked). `added`/`deleted` are parsed from `git diff --shortstat` on the
/// unstaged diff (the "what you'd commit if you staged everything" view);
/// untracked files don't contribute line counts because git doesn't diff them.
fn uncommitted_stats(repo_path: &Path) -> UncommittedStats {
    let files = count_changed_files(repo_path);
    let (added, deleted) = diff_shortstat(repo_path);
    UncommittedStats {
        files,
        added,
        deleted,
    }
}

/// Count entries in `git status --porcelain` (one per changed file).
fn count_changed_files(repo_path: &Path) -> u32 {
    let Some(mut cmd) = git_command(repo_path) else {
        return 0;
    };
    let output = cmd.args(["status", "--porcelain"]).output();
    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .count() as u32,
        Err(_) => 0,
    }
}

/// Parse `git diff --shortstat` into `(added, deleted)` line counts.
/// Output looks like ` 5 files changed, 120 insertions(+), 30 deletions(-)\n`
/// (any segment may be absent when its count is zero).
fn diff_shortstat(repo_path: &Path) -> (u32, u32) {
    let Some(mut cmd) = git_command(repo_path) else {
        return (0, 0);
    };
    let output = cmd.args(["diff", "--shortstat"]).output();
    let Ok(out) = output else {
        return (0, 0);
    };
    let line = String::from_utf8_lossy(&out.stdout);
    parse_shortstat(&line)
}

/// Parse a single `git diff --shortstat` line into `(added, deleted)`.
fn parse_shortstat(line: &str) -> (u32, u32) {
    let mut added = 0u32;
    let mut deleted = 0u32;
    for token in line.split(',') {
        let token = token.trim();
        if let Some(num) = parse_count_after(token, "insertion") {
            added = num;
        } else if let Some(num) = parse_count_after(token, "deletion") {
            deleted = num;
        }
    }
    (added, deleted)
}

/// If `token` matches `"<n> <key>[s] [(+)|(-)]"` (e.g. `"42 insertions(+)"`),
/// return the leading integer. Otherwise None.
fn parse_count_after(token: &str, key: &str) -> Option<u32> {
    let cleaned = token.trim();
    // Drop the trailing `(+)/(-)` marker.
    let rest = cleaned
        .strip_suffix("(+)")
        .or_else(|| cleaned.strip_suffix("(-)"))?;
    // `rest` now looks like " 42 insertions" — strip the trailing keyword
    // (singular or plural) and parse the leading number.
    let rest = rest.trim_end();
    let without_key = rest
        .strip_suffix(&format!("{key}s"))
        .or_else(|| rest.strip_suffix(key))?;
    without_key.trim().parse::<u32>().ok()
}

/// Get the ISO 8601 timestamp of the most recently modified file in the directory.
/// Skips `.git`, `node_modules`, `target`, and other large build directories.
fn last_modified_time(path: &Path) -> Option<String> {
    let mut newest: Option<std::time::SystemTime> = None;

    let is_ignored = |name: &str| -> bool {
        matches!(
            name,
            ".git"
                | "node_modules"
                | "target"
                | "__pycache__"
                | ".venv"
                | "venv"
                | "dist"
                | "build"
                | ".DS_Store"
        )
    };

    let walker = walkdir::WalkDir::new(path)
        .max_depth(8)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored(e.file_name().to_str().unwrap_or("")));

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = match metadata.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };

        match newest {
            None => newest = Some(modified),
            Some(ref current) if modified > *current => newest = Some(modified),
            _ => {}
        }
    }

    newest.and_then(|t| {
        t.duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn create_temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("loopdeck-git-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_no_git_directory() {
        let dir = create_temp_dir();
        fs::write(dir.join("README.md"), "# Test").unwrap();

        let info = check_git_info(&dir);
        assert!(info.last_commit_date.is_none());
        assert!(!info.is_dirty);
        // Should have a last_modified from the file we just wrote
        assert!(info.last_modified.is_some());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_git_repo_with_commits() {
        let dir = create_temp_dir();

        // Init git repo and make a commit
        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .arg("init")
            .output()
            .unwrap();

        // Configure git user for test
        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.email", "test@loopdeck.dev"])
            .output();
        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.name", "LoopDeck Test"])
            .output();

        // Create and commit a file
        let mut file = fs::File::create(dir.join("hello.txt")).unwrap();
        file.write_all(b"hello world").unwrap();

        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["add", "."])
            .output()
            .unwrap();
        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["commit", "-m", "initial commit"])
            .output()
            .unwrap();

        let info = check_git_info(&dir);
        assert!(info.last_commit_date.is_some());
        assert!(!info.is_dirty); // clean working tree

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn worktree_lifecycle_add_list_remove() {
        let dir = create_temp_dir();
        let linked = dir.with_extension("linked-worktree");
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .arg("init")
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.email", "test@loopdeck.dev"])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.name", "LoopDeck Test"])
            .output()
            .unwrap();
        fs::write(dir.join("README.md"), "# test\n").unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["commit", "-m", "initial"])
            .output()
            .unwrap();

        let added = worktree_add(&dir, &linked, "run/worktree-test").unwrap();
        assert_eq!(added.path, fs::canonicalize(&linked).unwrap());
        assert!(worktree_list(&dir).unwrap().contains(&added.path));
        worktree_remove(&dir, &added.path).unwrap();
        assert!(!linked.exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    /// Regression: a successful run leaves the worktree dirty (e.g. a
    /// post-push `.loopdeck/loops.md` update), and plain `git worktree
    /// remove` refuses dirty worktrees. Removal must be forced.
    #[test]
    fn worktree_remove_dirty_worktree() {
        let dir = create_temp_dir();
        let linked = dir.with_extension("dirty-linked-worktree");
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .arg("init")
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.email", "test@loopdeck.dev"])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.name", "LoopDeck Test"])
            .output()
            .unwrap();
        fs::write(dir.join("README.md"), "# test\n").unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["add", "."])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["commit", "-m", "initial"])
            .output()
            .unwrap();

        let added = worktree_add(&dir, &linked, "run/dirty-worktree-test").unwrap();
        // Dirty it the way a run does: untracked post-push write.
        fs::write(linked.join("uncommitted.txt"), "dirty").unwrap();

        worktree_remove(&dir, &added.path).expect("forced removal must succeed on dirty worktree");
        assert!(!linked.exists());

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_git_repo_with_uncommitted_changes() {
        let dir = create_temp_dir();

        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .arg("init")
            .output()
            .unwrap();

        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.email", "test@loopdeck.dev"])
            .output();
        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["config", "user.name", "LoopDeck Test"])
            .output();

        let mut file = fs::File::create(dir.join("committed.txt")).unwrap();
        file.write_all(b"committed").unwrap();

        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["add", "."])
            .output()
            .unwrap();
        let _ = Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["commit", "-m", "first"])
            .output()
            .unwrap();

        // Make an uncommitted change
        let mut file = fs::File::create(dir.join("uncommitted.txt")).unwrap();
        file.write_all(b"dirty").unwrap();

        let info = check_git_info(&dir);
        assert!(info.last_commit_date.is_some());
        assert!(info.is_dirty); // has untracked/uncommitted file

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_empty_directory_last_modified_none() {
        let dir = create_temp_dir();
        let info = check_git_info(&dir);
        // No files at all
        assert!(info.last_modified.is_none());
        fs::remove_dir_all(&dir).unwrap();
    }

    // ── UncommittedStats ──

    #[test]
    fn test_parse_shortstat_insertions_only() {
        assert_eq!(
            parse_shortstat(" 1 file changed, 42 insertions(+)"),
            (42, 0)
        );
    }

    #[test]
    fn test_parse_shortstat_both() {
        assert_eq!(
            parse_shortstat(" 3 files changed, 120 insertions(+), 17 deletions(-)"),
            (120, 17)
        );
    }

    #[test]
    fn test_parse_shortstat_single_insertion() {
        // git uses the singular form when count == 1
        assert_eq!(parse_shortstat(" 1 file changed, 1 insertion(+)"), (1, 0));
    }

    #[test]
    fn test_parse_shortstat_empty() {
        // clean tree → git prints an empty shortstat line
        assert_eq!(parse_shortstat(""), (0, 0));
    }

    #[test]
    fn test_uncommitted_stats_clean_tree() {
        let dir = create_temp_dir();
        init_repo(&dir);

        fs::write(dir.join("a.txt"), "a").unwrap();
        commit(&dir, "init");

        let stats = uncommitted_stats(&dir);
        assert_eq!(
            stats,
            UncommittedStats {
                files: 0,
                added: 0,
                deleted: 0
            }
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_uncommitted_stats_dirty_tree() {
        let dir = create_temp_dir();
        init_repo(&dir);

        fs::write(dir.join("a.txt"), "a\nb\nc\n").unwrap();
        commit(&dir, "init");

        // Edit tracked file (1 file, +1/-1 region) + add untracked (counts in files only)
        fs::write(dir.join("a.txt"), "a\nb\nCHANGED\n").unwrap();
        fs::write(dir.join("new.txt"), "fresh").unwrap();

        let stats = uncommitted_stats(&dir);
        assert_eq!(stats.files, 2, "tracked edit + untracked file");
        assert!(stats.added >= 1, "should report added lines");
        assert!(stats.deleted >= 1, "should report deleted lines");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_check_git_info_carries_uncommitted() {
        let dir = create_temp_dir();
        init_repo(&dir);
        fs::write(dir.join("a.txt"), "a").unwrap();
        commit(&dir, "init");

        // Clean → zero stats
        let info = check_git_info(&dir);
        assert_eq!(info.uncommitted, UncommittedStats::default());

        // Dirty → non-zero files
        fs::write(dir.join("b.txt"), "b").unwrap();
        let info = check_git_info(&dir);
        assert_eq!(info.uncommitted.files, 1);

        fs::remove_dir_all(&dir).unwrap();
    }

    // ── test helpers ──

    fn init_repo(dir: &Path) {
        let _ = Command::new("git")
            .args(["-C"])
            .arg(dir)
            .arg("init")
            .output()
            .unwrap();
        let _ = Command::new("git")
            .args(["-C"])
            .arg(dir)
            .args(["config", "user.email", "test@loopdeck.dev"])
            .output();
        let _ = Command::new("git")
            .args(["-C"])
            .arg(dir)
            .args(["config", "user.name", "LoopDeck Test"])
            .output();
    }

    fn commit(dir: &Path, msg: &str) {
        let _ = Command::new("git")
            .args(["-C"])
            .arg(dir)
            .args(["add", "."])
            .output()
            .unwrap();
        let _ = Command::new("git")
            .args(["-C"])
            .arg(dir)
            .args(["commit", "-m", msg])
            .output()
            .unwrap();
    }
}
