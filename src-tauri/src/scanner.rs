use crate::error::AppError;
use crate::git;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

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

/// Directories to skip during traversal.
const IGNORED_DIRS: &[&str] = &[
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
pub fn scan_directory(path: &Path, max_depth: u8) -> Result<Vec<DiscoveredRepo>, AppError> {
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

    // Walk the directory tree
    for entry in walkdir::WalkDir::new(&canonical_root)
        .max_depth(max_depth as usize)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored_dir(e.file_name().to_str().unwrap_or("")))
    {
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
            detected_stack,
            description_preview,
            last_commit: git_info.last_commit_date,
            last_modified: git_info.last_modified,
        });
    }

    // Sort by name
    repos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let elapsed = start.elapsed();
    tracing::info!(
        "Scanned {} — found {} repos in {:?}",
        canonical_root.display(),
        repos.len(),
        elapsed
    );

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
}
