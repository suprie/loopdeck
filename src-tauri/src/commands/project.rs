//! Project management commands: import/list/get/update/remove/open/rescan.

use super::state::{blocking_task_failed, derive_run_states, resolve_root, AppState};
use crate::config::{self, ProjectEntry, ProjectStatus, RunState};
use crate::error::AppError;
use crate::git;
use crate::graphify;
use crate::paths;
use crate::project::{self, ProjectMeta};
use crate::scanner;
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::State;
use tracing::{debug, info};

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

    // Canonicalize via the shared boundary helper (PRD FR3) so config lookups
    // use the canonical form. This is the registration path, so no registered-
    // root check — but the path must resolve to a real directory.
    let canonical = paths::canonical_root(&path)?;

    // Check if already registered (use canonical path for lookup). Done under a
    // brief lock so we don't hold the config mutex across the heavy bootstrapping
    // below — and so the early "already imported" return short-circuits before
    // any filesystem work.
    {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(existing) = config.find_by_path(&canonical) {
            return Ok(existing.clone());
        }
    }

    // Quick-scan for markers/README, bootstrap `.loopdeck/project.yaml`, gather
    // git info, and read the current loop. All blocking I/O — `quick_scan` +
    // `bootstrap_project` touch the filesystem and `check_git_info` spawns git
    // subprocesses — so it runs on the blocking pool, off the tokio worker.
    // `canonical` is cloned into the task; the outer value is retained to build
    // the registry entry after it completes.
    let (project_meta, git_info, current_loop) = tokio::task::spawn_blocking({
        let canonical = canonical.clone();
        move || -> Result<(project::ProjectMeta, git::GitInfo, Option<String>), AppError> {
            let (name, markers, has_readme) = scanner::quick_scan_directory(&canonical);
            let project_meta = project::bootstrap_project(&canonical, &name, &markers, has_readme)?;
            let git_info = git::check_git_info(&canonical);
            let current_loop = project::read_current_loop(canonical.as_path());
            Ok((project_meta, git_info, current_loop))
        }
    })
    .await
    .map_err(blocking_task_failed)??;

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
        uncommitted: git_info.uncommitted.into(),
        run_state: RunState::Idle,
        autonomous: false,
    };

    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        // Idempotent under a race: if a concurrent import registered this path
        // between our early check above and now, return its entry instead of
        // letting `add_project` error with `ProjectAlreadyExists`. Honors the
        // documented "already registered → return existing entry" contract.
        if let Some(existing) = config.find_by_path(&entry.path) {
            return Ok(existing.clone());
        }
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

    // Snapshot the project paths under a brief lock. We deliberately do NOT hold
    // the config mutex across the per-project git probing below — that probing
    // spawns subprocesses and walks each tree, which would block every other
    // command (and the whole tokio worker) for as long as it takes across every
    // registered project.
    let paths: Vec<PathBuf> = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        config.projects.iter().map(|e| e.path.clone()).collect()
    };

    // Refresh git info + current loop per project on the blocking pool. Results
    // are keyed by path so the apply pass stays aligned even if the registry
    // changed between snapshot and apply — a project added/removed by a
    // concurrent command simply won't match and is left untouched. Projects
    // whose path no longer exists are skipped (mirrors the prior inline guard).
    let refreshed: HashMap<PathBuf, (git::GitInfo, Option<String>)> =
        tokio::task::spawn_blocking(move || {
            let mut map = HashMap::with_capacity(paths.len());
            for path in paths {
                if !path.exists() {
                    continue;
                }
                let git_info = git::check_git_info(&path);
                let current_loop = project::read_current_loop(&path);
                map.insert(path, (git_info, current_loop));
            }
            map
        })
        .await
        .map_err(blocking_task_failed)?;

    // Apply the fresh data under a brief lock, persisting only if something
    // moved. The lock is held just for the mutation + save — not the git work.
    let mut out = {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        let mut changed = false;

        for entry in &mut config.projects {
            let Some((git_info, current_loop)) = refreshed.get(&entry.path) else {
                continue;
            };

            if entry.last_commit_date != git_info.last_commit_date {
                entry.last_commit_date = git_info.last_commit_date.clone();
                changed = true;
            }
            if entry.last_modified != git_info.last_modified {
                entry.last_modified = git_info.last_modified.clone();
                changed = true;
            }
            if entry.last_commit_message != git_info.last_commit_message {
                entry.last_commit_message = git_info.last_commit_message.clone();
                changed = true;
            }
            let fresh_uncommitted: config::UncommittedStats = git_info.uncommitted.into();
            if entry.uncommitted != fresh_uncommitted {
                entry.uncommitted = fresh_uncommitted;
                changed = true;
            }

            // Always re-read the current loop text. It isn't part of the
            // change/save decision — this matches the prior inline behaviour,
            // which set it unconditionally per project.
            entry.current_loop = current_loop.clone();

            // Recompute status from the (possibly refreshed) git dates so the
            // Dashboard reflects current freshness without a manual rescan.
            let before = entry.status;
            config::update_project_status(entry);
            if entry.status != before {
                changed = true;
            }
        }

        if changed {
            config.save()?;
        }

        config.projects.clone()
    };

    // Derive ephemeral run_state per project from live session + pending slots.
    // Done after the save (and outside the config lock — it only touches the
    // session/pending-slot maps) so transient state never reaches disk.
    derive_run_states(&state, &mut out);

    Ok(out)
}

/// Get a single project by path.
#[tauri::command]
pub async fn get_project(
    path: String,
    state: State<'_, AppState>,
) -> Result<ProjectEntry, AppError> {
    debug!("get_project called with path: {path}");
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    // Resolve the canonical, registered root (PRD FR3). Canonicalizing the
    // input *before* the lookup also fixes a latent mismatch: the registry
    // stores canonical paths, so a non-canonical input previously failed to
    // match and returned a spurious ProjectNotFound.
    let root = paths::resolve_registered_root(&config, &path)?;
    config
        .find_by_path(&root)
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

    // Resolve the canonical, registered root (PRD FR3) so both the
    // `.loopdeck/project.yaml` write and the registry update target a
    // registered project under its canonical path.
    let root = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };

    // Update the project.yaml file
    let meta = project::update_description(&root, &description)?;

    // Update in config registry
    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(entry) = config.find_by_path_mut(&root) {
            entry.description = description;
            config.save()?;
        }
    }

    info!("update_description complete for: {path}");
    Ok(meta)
}

/// Set the per-project autonomous-mode flag in the config registry.
///
/// When true, the project's LoopDeck-spawned agent self-approves
/// floor-clearing tool calls (Edit/Write, safe Bash, MCP, WebFetch) so loops
/// can run unattended — the user reviews the resulting PRs instead of each
/// tool call. The destructive floor (`rm -rf`, force-push, `curl|sh`, `sudo`,
/// …) still applies regardless. Takes effect on the next spawned session
/// (the policy is resolved at spawn time in `with_session`); a live session
/// keeps its current policy until reset.
#[tauri::command]
pub async fn set_project_autonomous(
    path: String,
    autonomous: bool,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("set_project_autonomous called for path: {path}, autonomous: {autonomous}");
    let root = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };
    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(entry) = config.find_by_path_mut(&root) {
            entry.autonomous = autonomous;
            config.save()?;
        }
    }
    info!("set_project_autonomous complete for: {path} → {autonomous}");
    Ok(())
}

/// Remove a project from the registry.
/// Does NOT delete the `.loopdeck/` directory or any project files.
#[tauri::command]
pub async fn remove_project(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("remove_project called for path: {path}");

    // Canonicalize via the shared boundary helper so the path matches the
    // stored canonical key. Registration isn't required here — this *is* the
    // deregistration — but the path must still resolve to a real directory.
    let canonical = paths::canonical_root(&path)?;

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
pub async fn open_in_finder(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("open_in_finder called for path: {path}");
    // Resolve the canonical, registered root (PRD FR3) before handing it to an
    // OS opener. `open`, `xdg-open`, and `explorer` otherwise interpret some
    // strings as URLs / scheme handlers — a registered, canonical directory
    // path blocks those tricks at the source.
    let resolved = resolve_root(&state, &path)?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Finder: {e}")))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open file manager: {e}")))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&resolved)
            .spawn()
            .map_err(|e| AppError::Scan(format!("Failed to open Explorer: {e}")))?;
    }

    info!("open_in_finder complete for: {path}");
    Ok(())
}

/// Open the repository path in the system terminal.
#[tauri::command]
pub async fn open_in_terminal(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("open_in_terminal called for path: {path}");
    let resolved = resolve_root(&state, &path)?;
    let path_str = resolved.to_string_lossy().to_string();

    #[cfg(target_os = "macos")]
    {
        use std::process::Stdio;

        // Try popular terminal apps in order of preference. The path is NEVER
        // interpolated into a shell/AppleScript string body: where a terminal
        // needs a working directory it is either passed as a dedicated argv
        // (Ghostty `--working-directory`, OSAScript `on run argv`) or set as
        // the spawned process's `current_dir`.
        fn ghostty(p: &str) -> std::process::Command {
            let mut cmd = std::process::Command::new("ghostty");
            cmd.arg(format!("--working-directory={p}"));
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            cmd
        }
        // OSAScript: read the path from argv and `cd` to its quoted form. The
        // path content is never embedded in the script source.
        fn iterm(p: &str) -> std::process::Command {
            let script = "on run argv\n tell application \"iTerm\" to create window with default profile command (\"cd \" & quoted form of (item 1 of argv))\nend run";
            let mut cmd = std::process::Command::new("osascript");
            cmd.args(["-e", script, p]);
            cmd
        }
        fn terminal_app(p: &str) -> std::process::Command {
            let script = "on run argv\n tell application \"Terminal\" to do script (\"cd \" & quoted form of (item 1 of argv))\nend run";
            let mut cmd = std::process::Command::new("osascript");
            cmd.args(["-e", script, p]);
            cmd
        }

        type Builder = fn(&str) -> std::process::Command;
        let builders: &[(&str, Builder)] = &[
            ("Ghostty", ghostty),
            ("iTerm", iterm),
            ("Terminal", terminal_app),
        ];

        let mut spawned = false;
        for (_name, builder) in builders {
            if builder(&path_str).spawn().is_ok() {
                spawned = true;
                break;
            }
        }

        if !spawned {
            // Last resort: just open the directory in Finder.
            std::process::Command::new("open")
                .arg(&resolved)
                .spawn()
                .map_err(|e| AppError::Scan(format!("Failed to open directory: {e}")))?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt; // enables Command::process_group()

        // Try common terminal emulators. Path is passed as a distinct argv,
        // never concatenated into a shell string.
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
            let mut cmd = std::process::Command::new(term);
            cmd.arg("--working-directory").arg(&path_str);
            // Detach the terminal into its own process group so it keeps running
            // independently of LoopDeck (won't receive SIGHUP when we exit).
            cmd.process_group(0);
            if cmd.spawn().is_ok() {
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
        // `current_dir` sets the working directory without injecting the path
        // into a `cd /d <path>` shell string (the previous form ran arbitrary
        // commands if `path` contained `&` or other metacharacters).
        std::process::Command::new("cmd")
            .arg("/K")
            .current_dir(&resolved)
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

    // Resolve the canonical, registered root under a brief lock (PRD FR3):
    // confirms the path exists, is a directory, and is in the registry. The
    // canonical form is what we probe with git and re-lookup in the apply
    // pass, so a non-canonical input matches the stored entry. We capture the
    // root and release the lock before the git subprocess runs — same
    // rationale as `list_projects`.
    let target = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };

    // Refresh git info on the blocking pool — spawns git subprocesses and walks
    // the tree for last-modified, so it must not run on the tokio worker. Clone
    // the root into the closure so the canonical path is still available for
    // the apply-pass lookup below.
    let target_for_probe = target.clone();
    let git_info = tokio::task::spawn_blocking(move || git::check_git_info(&target_for_probe))
        .await
        .map_err(blocking_task_failed)?;

    // Apply + persist under a brief lock. Note: `last_commit_message` is
    // intentionally not refreshed here — preserved from the prior behaviour.
    let mut result = {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        let entry = config
            .find_by_path_mut(&target)
            .ok_or(AppError::ProjectNotFound(path.clone()))?;
        entry.last_commit_date = git_info.last_commit_date.clone();
        entry.last_modified = git_info.last_modified.clone();
        entry.uncommitted = git_info.uncommitted.into();

        config::update_project_status(entry);

        let result = entry.clone();
        config.save()?;
        result
    };

    // Stamp the ephemeral run_state before returning so callers see the truth.
    let mut single = vec![result];
    derive_run_states(&state, &mut single);
    result = single.into_iter().next().unwrap();

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

    // Resolve the canonical, registered root (PRD FR3) before re-scanning and
    // rewriting `.loopdeck/project.yaml`.
    let root = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };
    let root_for_scan = root.clone();

    // Re-scan markers and README status for this repo
    let (name, markers, has_readme) = scanner::quick_scan_directory(&root_for_scan);
    let desc = project::regenerate_description(&root_for_scan, &name, &markers, has_readme)?;

    // Update in config registry
    {
        let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
        if let Some(entry) = config.find_by_path_mut(&root) {
            entry.description = desc.clone();
            config.save()?;
        }
    }

    info!("regenerate_description complete for: {path}");
    Ok(desc)
}

/// Summarize the Graphify knowledge graph for a project, if present.
///
/// Reads `graphify-out/graph.json` (and mines `graphify-out/GRAPH_REPORT.md`
/// for the build date) without ever running Graphify itself. Returns
/// `present: false` when the graph is missing or unparseable — the UI uses
/// that flag to hide the Graph tab rather than render empty stats.
///
/// Infallible beyond the standard `resolve_root` failure modes (unregistered
/// path, poisoned lock): a malformed `graph.json` is reported as
/// `present: false`, never as an `AppError`. This mirrors `get_decisions` /
/// `memory::parse_decisions` — on-disk-derived data degrades gracefully.
#[tauri::command]
pub async fn get_graphify_stats(
    path: String,
    state: State<'_, AppState>,
) -> Result<graphify::GraphifyStats, AppError> {
    debug!("get_graphify_stats called for path: {path}");

    // Resolve the canonical, registered root (PRD FR3) so the graph path is
    // constrained to a real project directory — no traversal outside it.
    let root = {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        paths::resolve_registered_root(&config, &path)?
    };

    // `read_stats` does file I/O (and on a real Graphify run the graph can be
    // a few hundred KB). Run on the blocking pool to stay off the tokio worker.
    let root_for_stats = root.clone();
    let stats = tokio::task::spawn_blocking(move || graphify::read_stats(&root_for_stats))
        .await
        .map_err(blocking_task_failed)?;
    Ok(stats)
}
