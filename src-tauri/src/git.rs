use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::time::UNIX_EPOCH;

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
}

/// Check git info and filesystem freshness for a directory.
pub fn check_git_info(path: &Path) -> GitInfo {
    let has_git = path.join(".git").exists();

    let (last_commit_date, last_commit_message, is_dirty) = if has_git {
        let last_commit_date = last_commit_date(path);
        let dirty = has_uncommitted_changes(path);
        let last_commit_message = last_commit_message(path);
        (last_commit_date, last_commit_message, dirty)
    } else {
        (None, None, false)
    };

    let last_modified = last_modified_time(path);
    GitInfo {
        last_commit_date,
        last_commit_message,
        is_dirty,
        last_modified,
    }
}
/// Get the commit message from last git commit.
fn last_commit_message(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo_path)
        .args(["log", "-1", "--format=%s"])
        .output()
        .ok()?;

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
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo_path)
        .args(["log", "-1", "--format=%cI"])
        .output()
        .ok()?;

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
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo_path)
        .args(["status", "--porcelain"])
        .output();

    match output {
        Ok(out) => !out.stdout.is_empty(),
        Err(_) => false,
    }
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
}
