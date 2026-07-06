use crate::config::{self, AgentConfig, GlobalConfig, ProjectEntry, ProjectStatus};
use crate::error::AppError;
use crate::git;
use crate::memory::{self, Decision, LoopStatus};
use crate::project::{self, ProjectMeta};
use crate::scanner::{self, DiscoveredRepo};
use chrono::Utc;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;
use tracing::{debug, info};

/// Shared application state managed by Tauri.
pub struct AppState {
    pub config: Mutex<GlobalConfig>,
}

/// Scan a directory for project repositories.
///
/// Recursively walks the directory tree looking for marker files
/// (`.git`, `Cargo.toml`, `package.json`, etc.). Returns discovered
/// repos with metadata — does NOT modify any files.
///
/// Cross-references with the global config: `has_loopdeck` is only
/// true if the project is actually registered, not just if a
/// `.loopdeck/` directory exists on disk.
#[tauri::command]
pub async fn scan_directory(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<DiscoveredRepo>, AppError> {
    debug!("scan_directory called with path: {path}");

    let scan_root = PathBuf::from(&path);
    let max_depth = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        config.settings.scan_depth
    };

    // Heavy I/O — scanner does recursive directory walking
    let mut repos = scanner::scan_directory(&scan_root, max_depth)?;

    // Cross-reference with global config: override `has_loopdeck` so it
    // reflects actual registration status, not just filesystem state.
    // This prevents repos that were removed from the registry but still
    // have a .loopdeck/ directory from appearing as "Imported".
    {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        for repo in &mut repos {
            repo.has_loopdeck = config.find_by_path(&repo.path).is_some();
        }
    }

    info!("scan_directory found {} repos", repos.len());
    Ok(repos)
}

/// Import a repository: bootstrap `.loopdeck/project.yaml` and register in global config.
///
/// If the repo is already registered, returns the existing entry.
/// If `.loopdeck/project.yaml` already exists, loads it instead of overwriting.
#[tauri::command]
pub async fn import_project(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectEntry, AppError> {
    debug!("import_project called with path: {path}");

    let repo_path = PathBuf::from(&path);

    // Canonicalize early so config lookups use the same path form
    let canonical = repo_path
        .canonicalize()
        .map_err(|e| AppError::Scan(format!("Failed to resolve path: {e}")))?;

    // Check if already registered (use canonical path for lookup)
    {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(existing) = config.find_by_path(&canonical) {
            return Ok(existing.clone());
        }
    }

    // Quick-scan the directory for markers and README
    let (name, markers, has_readme) = scanner::quick_scan_directory(&canonical);

    // Bootstrap .loopdeck/project.yaml
    let project_meta = project::bootstrap_project(&canonical, &name, &markers, has_readme)?;

    // Gather git info
    let git_info = git::check_git_info(&canonical);
    let current_loop = project::read_current_loop(canonical.as_path());

    // Build project entry and add to config
    let entry = ProjectEntry {
        path: canonical,
        name: project_meta.name,
        description: project_meta.description,
        status: ProjectStatus::Active,
        current_loop,
        last_opened: Some(Utc::now()),
        created_at: Utc::now(),
        last_commit_date: git_info.last_commit_date,
        last_commit_message: git_info.last_commit_message,
        last_modified: git_info.last_modified,
    };

    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        config.add_project(entry.clone())?;
        config.save()?;
    }

    info!("import_project complete: {entry:?}");
    Ok(entry)
}

/// List all registered projects.
#[tauri::command]
pub async fn list_projects(state: State<'_, AppState>) -> Result<Vec<ProjectEntry>, AppError> {
    debug!("list_projects called");
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let mut changed = false;

    // Refresh git info for each project whose path still exists
    for entry in &mut config.projects {
        // Skip projects whose path no longer exists
        if !entry.path.exists() {
            continue;
        }

        let git_info = git::check_git_info(&entry.path);

        if entry.last_commit_date != git_info.last_commit_date {
            entry.last_commit_date = git_info.last_commit_date;
            changed = true;
        }
        if entry.last_modified != git_info.last_modified {
            entry.last_modified = git_info.last_modified;
            changed = true;
        }
        if entry.last_commit_message != git_info.last_commit_message {
            entry.last_commit_message = git_info.last_commit_message;
            changed = true;
        }

        entry.current_loop = project::read_current_loop(entry.path.as_path());
    }

    if changed {
        config.save()?;
    }

    Ok(config.projects.clone())
}

/// Get the global agent configuration.
///
/// Returns `None` if no agent config has been saved yet.
#[tauri::command]
pub async fn get_agent_config(state: State<'_, AppState>) -> Result<Option<AgentConfig>, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    Ok(config.agent.clone())
}

/// Set (create or update) the global agent configuration.
///
/// Persists to `~/.config/loopdeck/config.yaml` and returns the saved config.
#[tauri::command]
pub async fn set_agent_config(
    agent_config: AgentConfig,
    state: State<'_, AppState>,
) -> Result<AgentConfig, AppError> {
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    config.agent = Some(agent_config.clone());
    config.save()?;
    Ok(agent_config)
}

/// Get a single project by path.
#[tauri::command]
pub async fn get_project(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectEntry, AppError> {
    debug!("get_project called with path: {path}");
    let repo_path = PathBuf::from(&path);
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    config
        .find_by_path(&repo_path)
        .cloned()
        .ok_or(AppError::ProjectNotFound(path))
}

/// Update the project description (both in `.loopdeck/project.yaml` and config registry).
#[tauri::command]
pub async fn update_description(
    path: String,
    description: String,
    state: State<'_, AppState>,
) -> Result<ProjectMeta, AppError> {
    debug!("update_description called for path: {path}");

    let repo_path = PathBuf::from(&path);

    // Update the project.yaml file
    let meta = project::update_description(&repo_path, &description)?;

    // Update in config registry
    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(entry) = config.find_by_path_mut(&repo_path) {
            entry.description = description;
            config.save()?;
        }
    }

    info!("update_description complete for: {path}");
    Ok(meta)
}

/// Remove a project from the registry.
/// Does NOT delete the `.loopdeck/` directory or any project files.
#[tauri::command]
pub async fn remove_project(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("remove_project called for path: {path}");

    let repo_path = PathBuf::from(&path);

    // Canonicalize so we match the stored path (which is always canonical)
    let canonical = repo_path
        .canonicalize()
        .map_err(|e| AppError::Scan(format!("Failed to resolve path: {e}")))?;

    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;

    if !config.remove_project(&canonical) {
        return Err(AppError::ProjectNotFound(path));
    }

    config.save()?;
    info!("remove_project complete for: {path}");
    Ok(())
}

/// Open the repository path in the system file manager (Finder on macOS).
#[tauri::command]
pub async fn open_in_finder(path: String) -> Result<(), AppError> {
    debug!("open_in_finder called for path: {path}");

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Finder: {e}")))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open file manager: {e}")))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Explorer: {e}")))?;
    }

    info!("open_in_finder complete for: {path}");
    Ok(())
}

/// Open the repository path in the system terminal.
#[tauri::command]
pub async fn open_in_terminal(path: String) -> Result<(), AppError> {
    debug!("open_in_terminal called for path: {path}");

    #[cfg(target_os = "macos")]
    {
        // Try popular terminal apps in order of preference.
        // Each app needs its own mechanism to open at a specific directory.
        let terminals: &[(&str, fn(&str) -> std::process::Command)] = &[
            // Ghostty: `open -a Ghostty --args --dir=<path>`
            ("Ghostty", |p| {
                use std::process::Stdio;

                let mut cmd = std::process::Command::new("ghostty");
                cmd.arg(format!("--working-directory={}", p));
                cmd.stdout(Stdio::null());
                cmd.stderr(Stdio::null());
                cmd
            }),
            // iTerm2: AppleScript to create a new window
            ("iTerm", |p| {
                let script = format!(
                    "tell application \"iTerm\" to create window with default profile command \"cd '{}' && clear\"",
                    p.replace('\'', "'\\''")
                );
                let mut cmd = std::process::Command::new("osascript");
                cmd.args(["-e", &script]);
                cmd
            }),
            // Terminal.app: AppleScript
            ("Terminal", |p| {
                let script = format!(
                    "tell application \"Terminal\" to do script \"cd '{}' && clear\"",
                    p.replace('\'', "'\\''")
                );
                let mut cmd = std::process::Command::new("osascript");
                cmd.args(["-e", &script]);
                cmd
            }),
        ];

        let mut spawned = false;
        for (_name, builder) in terminals {
            if let Ok(()) = builder(&path).spawn().map(|_| ()) {
                spawned = true;
                break;
            }
        }

        if !spawned {
            // Last resort: just open the directory in Finder
            std::process::Command::new("open")
                .arg(&path)
                .spawn()
                .map_err(|e| AppError::Scan(format!("Failed to open directory: {e}")))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Try common terminal emulators
        let terminals = [
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "x-terminal-emulator",
            "alacritty",
            "kitty",
        ];

        let mut spawned = false;
        for term in &terminals {
            if let Ok(mut child) = std::process::Command::new(term)
                .arg("--working-directory")
                .arg(&path)
                .spawn()
            {
                // Don't wait — let the terminal run independently
                let _ = child.process_group();
                spawned = true;
                break;
            }
        }

        if !spawned {
            return Err(AppError::Scan(
                "Could not find a terminal emulator. Please install gnome-terminal, konsole, or alacritty.".into(),
            ));
        }
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "cmd", "/K", &format!("cd /d {}", path)])
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Command Prompt: {e}")))?;
    }

    info!("open_in_terminal complete for: {path}");
    Ok(())
}

/// Rescan a single project to refresh git info (last commit, last modified).
/// Updates the project entry in the global config and returns it.
#[tauri::command]
pub async fn rescan_project(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectEntry, AppError> {
    debug!("rescan_project called for path: {path}");

    let repo_path = PathBuf::from(&path);
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;

    let entry = config
        .find_by_path_mut(&repo_path)
        .ok_or(AppError::ProjectNotFound(path.clone()))?;

    // Check if the path still exists
    if !entry.path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Project path no longer exists: {}",
            entry.path.display()
        )));
    }

    // Refresh git info
    let git_info = git::check_git_info(&entry.path);
    entry.last_commit_date = git_info.last_commit_date.clone();
    entry.last_modified = git_info.last_modified.clone();

    config::update_project_status(entry);

    let result = entry.clone();
    config.save()?;

    info!("rescan_project complete for: {path}");
    Ok(result)
}

/// Regenerate the project description by re-scanning README and structure.
/// Updates both `.loopdeck/project.yaml` and the config registry.
#[tauri::command]
pub async fn regenerate_description(
    path: String,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    debug!("regenerate_description called for path: {path}");

    let repo_path = PathBuf::from(&path);
    let repo_path_clone = repo_path.clone();

    // Re-scan markers and README status for this repo
    let (name, markers, has_readme) = scanner::quick_scan_directory(&repo_path_clone);
    let desc = project::regenerate_description(&repo_path_clone, &name, &markers, has_readme)?;

    // Update in config registry
    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(entry) = config.find_by_path_mut(&repo_path) {
            entry.description = desc.clone();
            config.save()?;
        }
    }

    info!("regenerate_description complete for: {path}");
    Ok(desc)
}

/// Get all decisions from `.loopdeck/decisions.md`.
/// Returns an empty list if the file does not exist.
#[tauri::command]
pub async fn get_decisions(
    path: String,
    _state: State<'_, AppState>,
) -> Result<Vec<Decision>, AppError> {
    debug!("get_decisions called for path: {path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let decisions = memory::parse_decisions(&repo_path);
    Ok(decisions)
}

/// Get loop status from `.loopdeck/loops.md`.
/// Returns an empty/default LoopStatus if the file does not exist.
#[tauri::command]
pub async fn get_loops(path: String, _state: State<'_, AppState>) -> Result<LoopStatus, AppError> {
    debug!("get_loops called for path: {path}");

    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let status = memory::parse_loops(&repo_path);
    Ok(status)
}
