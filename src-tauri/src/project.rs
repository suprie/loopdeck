use crate::error::AppError;
use crate::scanner::detect_stack;
use crate::skills;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Content of `.loopdeck/project.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub status: String,
    pub created_at: String,
}

impl ProjectMeta {
    /// Create a new ProjectMeta with defaults.
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            status: "active".to_string(),
            created_at: Utc::now().format("%Y-%m-%d").to_string(),
        }
    }
}

/// Bootstrap the `.loopdeck/project.yaml` file for a repository.
///
/// Creates `.loopdeck/` directory if absent, generates a description
/// from README.md (first paragraph) or falls back to repo structure,
/// and writes `project.yaml`.
///
/// Returns the `ProjectMeta` that was written.
pub fn bootstrap_project(
    repo_path: &Path,
    repo_name: &str,
    markers: &[String],
    has_readme: bool,
) -> Result<ProjectMeta, AppError> {
    eprintln!("Bootstraping project");
    let loopdeck_dir = repo_path.join(".loopdeck");
    let project_file = loopdeck_dir.join("project.yaml");
    let meta: ProjectMeta;
    // Check if already bootstrapped
    if !project_file.exists() {
        // Generate description
        create_files_and_directories(repo_path)?;
        let description = generate_description(repo_path, repo_name, markers, has_readme)?;
        setup_skills(repo_path, markers)?;
        meta = ProjectMeta::new(repo_name, &description);

        // Write project.yaml
        let contents = serde_yaml::to_string(&meta)?;
        std::fs::write(&project_file, contents)?;
        tracing::info!("Bootstrapped project at {:?}", repo_path.display());
    } else {
        create_files_and_directories(repo_path)?;
        meta = load_project(repo_path)?;
        setup_skills(repo_path, markers)?;
    }

    Ok(meta)
}

fn create_files_and_directories(repo_path: &Path) -> Result<(), AppError> {
    // Runtime layer.
    std::fs::create_dir_all(repo_path.join(".loopdeck"))?;
    // Spec layer — empty by default; epics are authored by humans / the
    // authoring skill, never seeded by the app.
    std::fs::create_dir_all(repo_path.join("docs").join("epics"))?;
    Ok(())
}

fn setup_skills(repo_path: &Path, markers: &[String]) -> Result<(), AppError> {
    // Copy relevant skill templates to .claude/skills/
    eprintln!("Copying skills");
    skills::copy_skills(repo_path, markers)?;

    // Set up hooks (Stop + PreToolUse) in .claude/settings.json
    skills::setup_hooks(repo_path)?;
    Ok(())
}

/// Load an existing `.loopdeck/project.yaml`.
pub fn load_project(repo_path: &Path) -> Result<ProjectMeta, AppError> {
    let project_file = repo_path.join(".loopdeck").join("project.yaml");

    if !project_file.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "No .loopdeck/project.yaml found at {}",
            repo_path.display()
        )));
    }

    let contents = std::fs::read_to_string(&project_file)?;
    let meta: ProjectMeta = serde_yaml::from_str(&contents)?;
    Ok(meta)
}

/// Update the description in an existing `.loopdeck/project.yaml`.
pub fn update_description(
    repo_path: &Path,
    new_description: &str,
) -> Result<ProjectMeta, AppError> {
    let loopdeck_dir = repo_path.join(".loopdeck");
    let project_file = loopdeck_dir.join("project.yaml");

    if !project_file.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "No .loopdeck/project.yaml found at {}",
            repo_path.display()
        )));
    }

    let contents = std::fs::read_to_string(&project_file)?;
    let mut meta: ProjectMeta = serde_yaml::from_str(&contents)?;
    meta.description = new_description.to_string();

    let updated = serde_yaml::to_string(&meta)?;
    std::fs::write(&project_file, updated)?;

    Ok(meta)
}

/// Regenerate the project description by re-scanning README and structure.
pub fn regenerate_description(
    repo_path: &Path,
    repo_name: &str,
    markers: &[String],
    has_readme: bool,
) -> Result<String, AppError> {
    let description = generate_description(repo_path, repo_name, markers, has_readme)?;

    // Update the project.yaml if it exists
    let project_file = repo_path.join(".loopdeck").join("project.yaml");
    if project_file.exists() {
        update_description(repo_path, &description)?;
    }

    Ok(description)
}

pub fn read_current_loop(path: &Path) -> Option<String> {
    let mut path: std::path::PathBuf = path.to_path_buf();
    path.push(".loopdeck");
    path.push("current-loop.md");
    if path.exists() {
        if let Ok(buf) = std::fs::read_to_string(&path) {
            let trimmed = buf.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

/// Generate a project description.
///
/// Strategy:
/// 1. If README exists: extract the first meaningful paragraph (after title, non-empty).
/// 2. Fallback: derive from detected technology stack and markers.
pub fn generate_description(
    repo_path: &Path,
    repo_name: &str,
    markers: &[String],
    has_readme: bool,
) -> Result<String, AppError> {
    if has_readme {
        if let Some(readme_description) = extract_readme_paragraph(repo_path)? {
            eprintln!("Readme {:?}", readme_description);
            return Ok(readme_description);
        }
    }

    // Fallback: generate description from repository structure
    let stack = detect_stack(markers);

    let description = if stack != "Unknown" {
        format!("A {stack} project.")
    } else if !markers.is_empty() {
        format!(
            "{name} — a project using {markers}.",
            name = repo_name,
            markers = markers.join(", ")
        )
    } else {
        format!("{name} — project repository.", name = repo_name)
    };

    Ok(description)
}

/// Extract the first meaningful paragraph from a README file.
///
/// Looks for README.md, README, or README.txt (in that order).
/// Strips leading `# ` headings, blank lines, and returns
/// the first non-heading paragraph of content.
fn extract_readme_paragraph(repo_path: &Path) -> Result<Option<String>, AppError> {
    // Try README variants in order of preference
    let readme_candidates = &["README.md", "README", "README.txt", "README.rst"];
    let mut readme_path: Option<PathBuf> = None;

    for candidate in readme_candidates {
        let path = repo_path.join(candidate);
        if path.exists() {
            readme_path = Some(path);
            break;
        }
    }

    let readme_path = match readme_path {
        Some(p) => p,
        None => return Ok(None),
    };

    let readme_content = std::fs::read_to_string(&readme_path)?;

    // Parse: find the first paragraph of meaningful content
    let mut in_code_block = false;
    let mut paragraph_lines: Vec<&str> = Vec::new();

    for line in readme_content.lines() {
        let trimmed = line.trim();

        // Track code blocks (``` or ~~~)
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            continue;
        }

        // Skip HTML comments
        if trimmed.starts_with("<!--") {
            continue;
        }

        // Skip headings
        if trimmed.starts_with('#') {
            // If we already collected paragraph, stop at next heading
            if !paragraph_lines.is_empty() {
                break;
            }
            continue;
        }

        // Skip horizontal rules
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            continue;
        }

        // Skip badges and images
        // Badges: [![...](https://...)](https://...) or [...](https://...) containing URLs
        if trimmed.starts_with("[!") && trimmed.contains("http") {
            continue;
        }
        if trimmed.starts_with("[") && trimmed.contains("http") && !trimmed.starts_with("[^") {
            continue;
        }
        if trimmed.starts_with("!") && trimmed.contains("http") {
            continue;
        }
        if trimmed.starts_with("<img") || trimmed.starts_with("<a href") {
            continue;
        }

        // Collect non-empty lines for the paragraph
        if trimmed.is_empty() {
            // Empty line — if we have content, end the paragraph
            if !paragraph_lines.is_empty() {
                break;
            }
        } else {
            // Only start collecting after we've seen a heading (or if right at start)
            paragraph_lines.push(trimmed);
        }
    }

    if paragraph_lines.is_empty() {
        // No heading found — take first non-empty lines as description
        for line in readme_content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with('#')
                || trimmed.starts_with("```")
                || trimmed.starts_with("<!--")
            {
                if !paragraph_lines.is_empty() {
                    break;
                }
                continue;
            }
            paragraph_lines.push(trimmed);
            // Collect up to 3 lines for the description preview
            if paragraph_lines.len() >= 3 {
                break;
            }
        }
    }

    if paragraph_lines.is_empty() {
        return Ok(None);
    }

    // Join lines into a single paragraph (markdown soft-wrap)
    let paragraph = paragraph_lines.join(" ");
    Ok(Some(paragraph))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_temp_repo() -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("loopdeck-proj-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        (dir, name)
    }

    #[test]
    fn test_bootstrap_creates_project_yaml() {
        let (dir, name) = create_temp_repo();
        let markers = vec!["Cargo.toml".to_string()];

        let meta = bootstrap_project(&dir, &name, &markers, false).unwrap();
        assert_eq!(meta.name, name);
        assert_eq!(meta.status, "active");
        assert!(meta.description.contains("Rust"));

        // Verify file exists and is valid
        let project_file = dir.join(".loopdeck").join("project.yaml");
        assert!(project_file.exists());
        let loaded: ProjectMeta =
            serde_yaml::from_str(&fs::read_to_string(&project_file).unwrap()).unwrap();
        assert_eq!(loaded.name, name);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_load_existing_project() {
        let (dir, name) = create_temp_repo();
        let markers = vec!["package.json".to_string()];

        bootstrap_project(&dir, &name, &markers, false).unwrap();
        let loaded = load_project(&dir).unwrap();
        assert_eq!(loaded.name, name);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_bootstrap_preserves_existing() {
        let (dir, name) = create_temp_repo();
        let markers = vec!["go.mod".to_string()];

        // First bootstrap
        let meta1 = bootstrap_project(&dir, &name, &markers, false).unwrap();

        // Second bootstrap should load existing (not overwrite)
        let meta2 = bootstrap_project(&dir, &name, &markers, false).unwrap();
        assert_eq!(meta1.description, meta2.description);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_update_description() {
        let (dir, name) = create_temp_repo();
        let markers = vec![".git".to_string()];

        bootstrap_project(&dir, &name, &markers, false).unwrap();
        let updated = update_description(&dir, "Updated description!").unwrap();
        assert_eq!(updated.description, "Updated description!");

        let loaded = load_project(&dir).unwrap();
        assert_eq!(loaded.description, "Updated description!");

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_load_nonexistent_project() {
        let result = load_project(&PathBuf::from("/tmp/does-not-exist-12345"));
        assert!(result.is_err());
        match result {
            Err(AppError::ProjectNotFound(_)) => {} // expected
            _ => panic!("Expected ProjectNotFound error"),
        }
    }

    #[test]
    fn test_readme_first_paragraph_extraction() {
        let (dir, name) = create_temp_repo();
        fs::write(
            dir.join("README.md"),
            "# My Project\n\nThis is the first paragraph of the project description.\n\n## Another section\n\nMore content here.\n",
        )
        .unwrap();

        let markers = vec!["Cargo.toml".to_string()];
        let description = generate_description(&dir, &name, &markers, true).unwrap();
        assert_eq!(
            description,
            "This is the first paragraph of the project description."
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_readme_with_badges_skipped() {
        let (dir, name) = create_temp_repo();
        fs::write(
            dir.join("README.md"),
            "# App Name\n\n[![Build](https://img.shields.io/badge/build-passing-brightgreen)](https://ci.example.com)\n\nThis is the actual first paragraph.\n\nMore text.\n",
        )
        .unwrap();

        let markers = vec!["package.json".to_string()];
        let description = generate_description(&dir, &name, &markers, true).unwrap();
        // Should skip the badge line and pick the actual text
        assert!(description.contains("actual first paragraph"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_no_readme_fallback_to_stack() {
        let (dir, name) = create_temp_repo();
        let markers = vec!["Cargo.toml".to_string()];

        let description = generate_description(&dir, &name, &markers, false).unwrap();
        assert!(description.contains("Rust"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_no_readme_no_markers_fallback() {
        let (dir, name) = create_temp_repo();

        let description = generate_description(&dir, &name, &[], false).unwrap();
        assert!(description.contains("project repository"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_readme_code_block_skipped() {
        let (dir, name) = create_temp_repo();
        fs::write(
            dir.join("README.md"),
            "# Tool\n\n```rust\nfn main() {}\n```\n\nThis is the description after the code block.\n\nMore text.",
        )
        .unwrap();

        let markers: Vec<String> = vec![];
        let description = generate_description(&dir, &name, &markers, true).unwrap();
        assert!(description.contains("description after the code block"));

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_bootstrap_copies_skills_and_hooks() {
        let (dir, name) = create_temp_repo();
        let markers = vec!["Cargo.toml".to_string(), "package.json".to_string()];
        // Create src-tauri/ so Tauri detection passes
        fs::create_dir_all(dir.join("src-tauri")).unwrap();

        bootstrap_project(&dir, &name, &markers, false).unwrap();

        // Verify skills were copied
        let skills_dir = dir.join(".claude").join("skills");
        assert!(skills_dir.exists());
        assert!(skills_dir
            .join("loopdeck-orchestrator")
            .join("SKILL.md")
            .exists());
        assert!(skills_dir
            .join("loopdeck-rust-expert")
            .join("SKILL.md")
            .exists());
        assert!(skills_dir
            .join("loopdeck-vite-senior-engineer")
            .join("SKILL.md")
            .exists());
        assert!(skills_dir
            .join("loopdeck-tauri-expert")
            .join("SKILL.md")
            .exists());

        // Verify hooks were set up (Claude Code format: matcher groups -> hooks array)
        let settings_path = dir.join(".claude").join("settings.json");
        assert!(settings_path.exists());
        let raw = fs::read_to_string(&settings_path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&raw).unwrap();

        // Stop: 1 matcher group, 2 command hooks inside
        let stop_groups = root["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop_groups.len(), 1);
        let stop_hooks = stop_groups[0]["hooks"].as_array().unwrap();
        assert_eq!(stop_hooks.len(), 2);

        // PreToolUse: 1 matcher group with matcher "Skill", 1 command hook inside
        let ptuse_groups = root["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(ptuse_groups.len(), 1);
        assert_eq!(ptuse_groups[0]["matcher"].as_str().unwrap(), "Skill");
        let ptuse_hooks = ptuse_groups[0]["hooks"].as_array().unwrap();
        assert_eq!(ptuse_hooks.len(), 1);
        assert_eq!(
            ptuse_hooks[0]["command"].as_str().unwrap(),
            "python3 .loopdeck/hooks/orchestrator-start.py"
        );

        // Verify hook scripts were copied to .loopdeck/hooks/
        let hooks_dir = dir.join(".loopdeck").join("hooks");
        assert!(hooks_dir.join("loopdeck-stop-hook.py").exists());
        assert!(hooks_dir.join("loopdeck-memory-write.sh").exists());
        assert!(hooks_dir.join("orchestrator-start.py").exists());

        fs::remove_dir_all(&dir).unwrap();
    }
}
