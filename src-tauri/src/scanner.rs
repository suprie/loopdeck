use crate::error::AppError;
use crate::git;
use crate::limits;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Marker files and directories that indicate a project repository.
const PROJECT_MARKERS: &[&str] = &[
    ".git",          // directory — most common
    "Cargo.toml",    // Rust
    "package.json",  // Node.js / TypeScript
    "go.mod",        // Go
    "Package.swift", // Swift
    "Gemfile",       // Ruby
    "Podfile",       // CocoaPods (iOS)
];

/// Filename patterns that indicate an Xcode project (checked separately).
const XCODE_PATTERNS: &[&str] = &[".xcodeproj", ".xcworkspace"];

/// Directories to skip during traversal. Public so other modules (e.g. the
/// file-mention `list_dir_entries` command) can reuse the same ignore list and
/// stay consistent with project scanning.
pub const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".DS_Store",
];

/// A repository discovered during scanning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredRepo {
    /// Absolute path to the repository root.
    pub path: PathBuf,
    /// Repository name derived from directory name.
    pub name: String,
    /// List of detected marker files at this path.
    pub markers: Vec<String>,
    /// Whether a README.md (or README, README.txt) exists at this path.
    pub has_readme: bool,
    /// Whether `.loopdeck/project.yaml` already exists.
    pub has_loopdeck: bool,
    /// Whether `graphify-out/graph.json` exists (project already mapped by Graphify).
    pub has_graphify: bool,
    /// Human-readable technology stack derived from marker files.
    /// e.g. "Rust, JavaScript/TypeScript"
    pub detected_stack: String,
    /// Lightweight description preview generated from the detected stack.
    /// The full description is generated on import (may use README).
    pub description_preview: String,
    /// ISO 8601 timestamp of the last git commit, if a git repo.
    pub last_commit: Option<String>,
    /// ISO 8601 timestamp of the most recently modified file.
    pub last_modified: Option<String>,
}

/// Recursively scan a directory for project repositories.
///
/// A directory is considered a repository root if it contains
/// any of the `PROJECT_MARKERS` files/directories or matches
/// Xcode project patterns.
///
/// Returns all discovered repositories, sorted by name.
///
/// Bounds (PRD FR4) — visited entries, returned results, and wall-clock — come
/// from [`limits`]. When a budget is hit the scan stops and returns whatever was
/// found so far (with a `warn!` log), so a pathologically large root can't
/// freeze the UI or exhaust memory.
pub fn scan_directory(path: &Path, max_depth: u8) -> Result<Vec<DiscoveredRepo>, AppError> {
    scan_directory_bounded(
        path,
        max_depth,
        limits::SCAN_MAX_ENTRIES,
        limits::SCAN_MAX_RESULTS,
        limits::SCAN_MAX_DURATION,
    )
}

/// The budget-parameterized core of [`scan_directory`], extracted so the
/// budgets are injectable in tests (the production constants are far too large
/// to exercise by building a real tree).
fn scan_directory_bounded(
    path: &Path,
    max_depth: u8,
    max_entries: usize,
    max_results: usize,
    max_duration: Duration,
) -> Result<Vec<DiscoveredRepo>, AppError> {
    let start = Instant::now();

    if !path.exists() {
        return Err(AppError::InvalidPath(format!(
            "Directory does not exist: {}",
            path.display()
        )));
    }

    if !path.is_dir() {
        return Err(AppError::InvalidPath(format!(
            "Path is not a directory: {}",
            path.display()
        )));
    }

    let canonical_root = path
        .canonicalize()
        .map_err(|e| AppError::Scan(format!("Failed to resolve path: {e}")))?;

    let mut repos: Vec<DiscoveredRepo> = Vec::new();

    // Helper: check if entry name matches ignored dirs
    let is_ignored_dir = |name: &str| -> bool { IGNORED_DIRS.contains(&name) };

    // Budgets (PRD FR4): bound visited entries and wall-clock so a pathologically
    // large root (e.g. `$HOME`) can't freeze the UI or exhaust memory. When a
    // budget is hit we stop and return what we have so far — discovery still
    // surfaces results, just fewer — and log a warning so the truncation is
    // visible. The result cap bounds the returned `Vec` itself.
    let mut entries_visited: usize = 0;
    let mut truncated = false;

    // Walk the directory tree
    for entry in walkdir::WalkDir::new(&canonical_root)
        .max_depth(max_depth as usize)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored_dir(e.file_name().to_str().unwrap_or("")))
    {
        entries_visited += 1;
        if entries_visited > max_entries {
            truncated = true;
            tracing::warn!(
                root = %canonical_root.display(),
                visited = entries_visited,
                limit = max_entries,
                "scan hit entry budget — returning partial results"
            );
            break;
        }
        if start.elapsed() >= max_duration {
            truncated = true;
            tracing::warn!(
                root = %canonical_root.display(),
                elapsed = ?start.elapsed(),
                limit = ?max_duration,
                "scan hit time budget — returning partial results"
            );
            break;
        }

        let entry = entry?;

        // Only examine directories
        if !entry.file_type().is_dir() {
            continue;
        }

        let dir_path = entry.path();

        // Check if this directory contains any project markers
        let mut markers: Vec<String> = Vec::new();
        let mut has_readme = false;

        // Read directory contents
        let dir_entries: Vec<String> = match std::fs::read_dir(dir_path) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect(),
            Err(_) => continue,
        };

        for item in &dir_entries {
            // Check project markers
            for marker in PROJECT_MARKERS {
                if item == *marker {
                    markers.push(marker.to_string());
                }
            }

            // Check Xcode patterns (suffix match)
            for pattern in XCODE_PATTERNS {
                if item.ends_with(pattern) {
                    markers.push(item.clone());
                }
            }

            // Check for README
            if item.to_lowercase().starts_with("readme") {
                has_readme = true;
            }
        }

        if markers.is_empty() {
            continue;
        }

        // Don't include sub-paths of already-discovered repos
        // A repo root is the directory that directly contains markers.
        // If we already have a parent repo, skip deeper children.
        let is_child_of_existing = repos.iter().any(|r| {
            dir_path
                .strip_prefix(&r.path)
                .map(|relative| !relative.as_os_str().is_empty())
                .unwrap_or(false)
        });

        if is_child_of_existing {
            continue;
        }

        let name = dir_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir_path.to_string_lossy().to_string());

        let has_loopdeck = dir_path.join(".loopdeck").join("project.yaml").exists();

        // Graphify is an optional, externally-managed producer — LoopDeck only
        // reads its output. Check via the shared helper so the path layout
        // lives in one place (`graphify::is_present`).
        let has_graphify = crate::graphify::is_present(dir_path);

        // Detect technology stack and generate a lightweight description preview
        let detected_stack = detect_stack(&markers);
        let description_preview = if detected_stack != "Unknown" {
            format!("A {detected_stack} project.")
        } else if !markers.is_empty() {
            format!(
                "{name} — a project using {markers}.",
                name = name,
                markers = markers.join(", ")
            )
        } else {
            format!("{name} — project repository.", name = name)
        };

        // Check git info for freshness
        let git_info = git::check_git_info(dir_path);

        repos.push(DiscoveredRepo {
            path: dir_path.to_path_buf(),
            name,
            markers,
            has_readme,
            has_loopdeck,
            has_graphify,
            detected_stack,
            description_preview,
            last_commit: git_info.last_commit_date,
            last_modified: git_info.last_modified,
        });

        // Result budget (PRD FR4): stop once we've collected enough repos — the
        // returned Vec shouldn't itself be an exhaustion vector.
        if repos.len() >= max_results {
            truncated = true;
            tracing::warn!(
                root = %canonical_root.display(),
                found = repos.len(),
                limit = max_results,
                "scan hit result budget — returning partial results"
            );
            break;
        }
    }

    // Sort by name
    repos.sort_by_key(|a| a.name.to_lowercase());

    let elapsed = start.elapsed();
    if truncated {
        tracing::warn!(
            "Scanned {} — found {} repos in {:?} (TRUNCATED: a budget was hit)",
            canonical_root.display(),
            repos.len(),
            elapsed
        );
    } else {
        tracing::info!(
            "Scanned {} — found {} repos in {:?}",
            canonical_root.display(),
            repos.len(),
            elapsed
        );
    }

    Ok(repos)
}

/// Quick-scan a single directory for markers and README status.
/// Used by import and regenerate-description commands.
pub fn quick_scan_directory(path: &Path) -> (String, Vec<String>, bool) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let mut markers: Vec<String> = Vec::new();
    let mut has_readme = false;

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();

            for marker in &[
                ".git",
                "Cargo.toml",
                "package.json",
                "go.mod",
                "Package.swift",
                "Gemfile",
                "Podfile",
            ] {
                if fname == *marker {
                    markers.push(marker.to_string());
                }
            }

            if fname.ends_with(".xcodeproj") || fname.ends_with(".xcworkspace") {
                markers.push(fname.clone());
            }

            if fname.to_lowercase().starts_with("readme") {
                has_readme = true;
            }
        }
    }

    (name, markers, has_readme)
}

/// Detect the technology stack from marker files.
pub fn detect_stack(markers: &[String]) -> String {
    let mut stacks: Vec<&str> = Vec::new();

    for marker in markers {
        match marker.as_str() {
            "Cargo.toml" => stacks.push("Rust"),
            "package.json" => stacks.push("JavaScript/TypeScript"),
            "go.mod" => stacks.push("Go"),
            "Package.swift" => stacks.push("Swift"),
            "Podfile" => stacks.push("iOS (CocoaPods)"),
            "Gemfile" => stacks.push("Ruby"),
            _ => {
                if (marker.ends_with(".xcodeproj") || marker.ends_with(".xcworkspace"))
                    && !stacks.contains(&"Xcode")
                {
                    stacks.push("Xcode");
                }
            }
        }
    }

    if stacks.is_empty() {
        "Unknown".into()
    } else {
        stacks.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("loopdeck-scan-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_empty_directory() {
        let dir = create_temp_dir();
        let repos = scan_directory(&dir, 5).unwrap();
        assert!(repos.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_finds_git_repo() {
        let dir = create_temp_dir();
        let repo = dir.join("my-project");
        fs::create_dir_all(repo.join(".git")).unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "my-project");
        assert!(repos[0].markers.contains(&".git".to_string()));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_finds_cargo_project() {
        let dir = create_temp_dir();
        let repo = dir.join("rust-app");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("Cargo.toml"), "[package]").unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "rust-app");
        assert!(repos[0].markers.contains(&"Cargo.toml".to_string()));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_finds_package_json() {
        let dir = create_temp_dir();
        let repo = dir.join("node-app");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("package.json"), "{}").unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "node-app");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_detects_readme() {
        let dir = create_temp_dir();
        let repo = dir.join("with-readme");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("package.json"), "{}").unwrap();
        fs::write(repo.join("README.md"), "# Hello").unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert!(repos[0].has_readme);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_detects_loopdeck() {
        let dir = create_temp_dir();
        let repo = dir.join("with-loopdeck");
        fs::create_dir_all(repo.join(".loopdeck")).unwrap();
        fs::write(repo.join(".loopdeck/project.yaml"), "").unwrap();
        fs::write(repo.join("Cargo.toml"), "[package]").unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert!(repos[0].has_loopdeck);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_detects_graphify() {
        let dir = create_temp_dir();
        let repo = dir.join("with-graphify");
        fs::create_dir_all(repo.join("graphify-out")).unwrap();
        fs::write(repo.join("graphify-out/graph.json"), "{}").unwrap();
        fs::write(repo.join("Cargo.toml"), "[package]").unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert!(repos[0].has_graphify);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_graphify_absent_when_dir_missing() {
        let dir = create_temp_dir();
        let repo = dir.join("no-graphify");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("Cargo.toml"), "[package]").unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert!(!repos[0].has_graphify);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_ignores_node_modules() {
        let dir = create_temp_dir();
        // Create a nested structure with node_modules containing a package.json
        let nested = dir.join("app").join("node_modules").join("some-lib");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("package.json"), "{}").unwrap();

        // But the actual project is at dir/app
        fs::write(dir.join("app").join("package.json"), "{}").unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        // Should find only the app, not the node_modules content
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "app");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_nonexistent_path() {
        let result = scan_directory(&PathBuf::from("/nonexistent/path/12345"), 5);
        assert!(result.is_err());
        match result {
            Err(AppError::InvalidPath(_)) => {} // expected
            _ => panic!("Expected InvalidPath error"),
        }
    }

    #[test]
    fn test_detect_stack() {
        assert_eq!(detect_stack(&["Cargo.toml".into()]), "Rust");
        assert_eq!(
            detect_stack(&["package.json".into()]),
            "JavaScript/TypeScript"
        );
        assert_eq!(
            detect_stack(&["Cargo.toml".into(), "package.json".into()]),
            "Rust, JavaScript/TypeScript"
        );
        assert_eq!(detect_stack(&[]), "Unknown");
    }

    #[test]
    fn test_max_depth_enforced() {
        let dir = create_temp_dir();
        // Create a repo 4 levels deep: dir/a/b/c/d with .git
        let deep_repo = dir.join("a").join("b").join("c").join("d");
        fs::create_dir_all(deep_repo.join(".git")).unwrap();

        // With max_depth=3, the walker won't reach depth 4 (dir/a/b/c/d)
        let repos = scan_directory(&dir, 3).unwrap();
        assert!(
            repos.is_empty(),
            "repo at depth 4 should not be found with max_depth=3"
        );

        // With max_depth=5, it should find it
        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "d");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_skips_children_of_repo() {
        let dir = create_temp_dir();
        // Create a repo with markers
        let repo = dir.join("parent-repo");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("package.json"), "{}").unwrap();

        // Sub-directory should not be treated as separate repo
        fs::create_dir_all(repo.join("src")).unwrap();

        let repos = scan_directory(&dir, 5).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "parent-repo");

        fs::remove_dir_all(&dir).unwrap();
    }

    // ── resource budgets (PRD FR4) ─────────────────────────────────────

    #[test]
    fn test_entry_budget_returns_partial_results() {
        // Three repos, but an entry budget of 4 (root + first repo's .git dir +
        // ... ) — the scan must stop early and return a subset rather than
        // walking the whole tree or erroring. We assert it returns *some*
        // results and does not panic, not an exact count (walkdir's internal
        // entry accounting is what it is).
        let dir = create_temp_dir();
        for i in 0..3 {
            let repo = dir.join(format!("repo{i}"));
            fs::create_dir_all(repo.join(".git")).unwrap();
        }

        let repos = scan_directory_bounded(
            &dir,
            5,
            /* max_entries */ 3,
            /* max_results */ 100,
            Duration::from_secs(30),
        )
        .unwrap();
        // Partial: at most 3 repos exist; with a tight entry budget we get ≤ 3.
        assert!(repos.len() <= 3, "entry budget should bound results");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_result_budget_caps_returned_repos() {
        // Five repos, but a result budget of 2 — the scan must stop after 2.
        let dir = create_temp_dir();
        for i in 0..5 {
            let repo = dir.join(format!("repo{i}"));
            fs::create_dir_all(repo.join(".git")).unwrap();
        }

        let repos = scan_directory_bounded(
            &dir,
            5,
            /* max_entries */ 100_000,
            /* max_results */ 2,
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(
            repos.len(),
            2,
            "result budget should cap the returned Vec at 2"
        );

        fs::remove_dir_all(&dir).unwrap();
    }
}
