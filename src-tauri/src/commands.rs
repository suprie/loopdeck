use crate::agents::AgentResponse;
use crate::agents::ClaudeEvent;
use crate::claude_session::ClaudeSession;
use crate::config::{self, AgentConfig, GlobalConfig, ProjectEntry, ProjectStatus};
use crate::conversation::{self, ConversationTurn};
use crate::error::AppError;
use crate::git;
use crate::memory::{self, Decision, LoopStatus};
use crate::project::{self, ProjectMeta};
use crate::scanner::{self, DiscoveredRepo};
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;
use tracing::{debug, info};

/// Shared application state managed by Tauri.
///
/// `claude_sessions` uses a two-layer lock so projects run concurrently while
/// turns within one project serialize (one process, one stdin):
/// - **Outer** `std::sync::Mutex` guards the map for insert/lookup only —
///   held for microseconds, NEVER across `.await` (would deadlock / is unsound
///   across threads). The guard is dropped before any async work.
/// - **Inner** `tokio::sync::Mutex` per project, held for one full turn
///   (seconds–minutes). Different projects take different inner locks, so
///   they run in true parallel.
pub struct AppState {
    pub config: Mutex<GlobalConfig>,
    pub claude_sessions: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<ClaudeSession>>>>,
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

// ── Agent commands ─────────────────────────────────────────────────────────

/// Start the next development loop for a project.
///
/// Builds the next-loop prompt from `.loopdeck/loops.md` (first unchecked
/// step under `## Next Steps`, or a "propose the next loop" fallback) and
/// sends it through the shared agent pipeline. The turn is recorded to the
/// conversation transcript; the live process is spawned/resumed as needed.
#[tauri::command]
pub async fn agent_start_loop(
    path: String,
    state: State<'_, AppState>,
) -> Result<AgentResponse, AppError> {
    debug!("agent_start_loop called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!("Path does not exist: {path}")));
    }

    let prompt = build_next_loop_prompt(&repo_path);
    let response = send_and_record(&state, &repo_path, &prompt).await?;
    info!("agent_start_loop complete for: {path}");
    Ok(response)
}

/// Send a free-form follow-up message to the project's agent session.
///
/// Same shared pipeline as `agent_start_loop` — the live process is reused if
/// present, spawned/resumed otherwise, and both turns are recorded.
#[tauri::command]
pub async fn agent_send_message(
    path: String,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<AgentResponse, AppError> {
    debug!("agent_send_message called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!("Path does not exist: {path}")));
    }

    let response = send_and_record(&state, &repo_path, &prompt).await?;
    info!("agent_send_message complete for: {path}");
    Ok(response)
}

/// Start the next development loop with streaming events via Tauri Channel.
///
/// Like `agent_start_loop`, but emits each assistant content block as a
/// `ClaudeEvent` on the `on_event` channel as it arrives. The transcript is
/// still recorded atomically after the turn completes.
///
/// Returns `()` rather than `AgentResponse` — the terminal result event
/// (`ClaudeEvent::Result`) carries the full aggregated response.
#[tauri::command]
pub async fn agent_start_loop_streaming(
    path: String,
    on_event: Channel<ClaudeEvent>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("agent_start_loop_streaming called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!("Path does not exist: {path}")));
    }

    let prompt = build_next_loop_prompt(&repo_path);
    send_and_record_streaming(&state, &repo_path, &prompt, &on_event).await?;
    info!("agent_start_loop_streaming complete for: {path}");
    Ok(())
}

/// Send a free-form follow-up message with streaming events via Tauri Channel.
///
/// Like `agent_send_message`, but each assistant content block is emitted as
/// a `ClaudeEvent` on the `on_event` channel as it arrives, so the frontend
/// can render tokens immediately. The transcript is still recorded atomically
/// after the turn completes.
///
/// Returns `()` rather than `AgentResponse` — the terminal result event
/// (`ClaudeEvent::Result`) carries the full aggregated response, so the
/// frontend doesn't need to await the return value.
#[tauri::command]
pub async fn agent_send_message_streaming(
    path: String,
    prompt: String,
    on_event: Channel<ClaudeEvent>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("agent_send_message_streaming called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!("Path does not exist: {path}")));
    }

    send_and_record_streaming(&state, &repo_path, &prompt, &on_event).await?;
    info!("agent_send_message_streaming complete for: {path}");
    Ok(())
}

/// Load the persisted conversation transcript for the Agent tab.
///
/// Returns an empty vec when no conversation has been recorded yet. Lenient
/// about malformed lines (a corrupt append doesn't hide earlier turns).
#[tauri::command]
pub async fn agent_get_conversation(
    path: String,
    _state: State<'_, AppState>,
) -> Result<Vec<ConversationTurn>, AppError> {
    debug!("agent_get_conversation called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!("Path does not exist: {path}")));
    }
    Ok(conversation::load_conversation(&repo_path))
}

/// Reset the project's agent session: drop the live process and archive the
/// transcript.
///
/// The next `agent_start_loop` starts a fresh conversation (no `--resume`).
/// The archived transcript is preserved as `archive-<ts>.jsonl` for history.
#[tauri::command]
pub async fn agent_reset_session(
    path: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!("agent_reset_session called for path: {path}");
    let repo_path = PathBuf::from(&path);

    // Remove the live session from the map. The Arc's last reference drops
    // here → `ClaudeSession::Drop` closes stdin and reaps the child.
    let removed = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(&repo_path)
        .is_some();
    if removed {
        debug!("dropped live claude session for: {path}");
    }

    // Archive the transcript regardless of whether a live session existed —
    // a reset should always mean "next Start is fresh".
    conversation::archive_conversation(&repo_path)?;

    info!("agent_reset_session complete for: {path}");
    Ok(())
}

// ── Agent session helpers ──────────────────────────────────────────────────
//
// `with_session` is the single chokepoint through which every agent turn
// flows. It owns the get-or-spawn + lock lifecycle so commands stay thin and
// the per-project turn lock invariant (one stdin, one process) can't be
// violated by a caller.

/// Get the live `ClaudeSession` for `path` as an owned `Arc`, spawning one if
/// none exists (or resuming a prior conversation via `--resume`).
///
/// The caller `.lock().await`s the Arc to take the per-project turn lock — so
/// turns within one project serialize, while different projects (different map
/// entries, different inner locks) run concurrently. Returning the Arc (not a
/// guard) avoids lifetime tangles: the Arc is owned and stays referenced in
/// the map, keeping the process alive between turns.
///
/// # Lock invariant
/// The outer `std::sync::Mutex` guard is dropped **before** the caller awaits
/// the inner lock. Holding the std Mutex across `.await` would deadlock (or, if
/// sent across threads, be unsound). The map guard is a plainly-scoped
/// block-local that ends inside this function, before it returns.
async fn with_session(
    state: &AppState,
    path: &Path,
) -> Result<Arc<tokio::sync::Mutex<ClaudeSession>>, AppError> {
    // ── Outer (map) lock: held only to read/insert the Arc. ──
    let mut map_guard = state.claude_sessions.lock().map_err(|_| AppError::LockError)?;

    if let Some(arc) = map_guard.get(path) {
        // Existing live session — clone the Arc and reuse it. No .await here.
        return Ok(Arc::clone(arc));
    }

    // No live session — spawn one. Read agent config + resume id while we
    // still hold the map lock cheaply (still no .await in this scope).
    let agent_config = state
        .config
        .lock()
        .map_err(|_| AppError::LockError)?
        .agent
        .clone()
        .ok_or_else(|| {
            AppError::Agent(
                "no agent config set; configure it in Settings before starting a loop".into(),
            )
        })?;

    let resume_id = conversation::last_session_id(path);
    let session = ClaudeSession::spawn(&path.to_path_buf(), &agent_config, resume_id.as_deref())?;
    let arc = Arc::new(tokio::sync::Mutex::new(session));
    map_guard.insert(path.to_path_buf(), Arc::clone(&arc));
    // ── map_guard (std Mutex) dropped here as this scope ends. ──
    Ok(arc)
}

/// Build the prompt that kicks off the next development loop.
///
/// Scans `.loopdeck/loops.md` raw text for the first unchecked `- [ ]` under
/// `## Next Steps` (the structured `memory::parse_loops` flattens checked and
/// unchecked steps together, so we read the raw file here to preserve the
/// distinction). Falls back to a "propose the next loop" prompt when every
/// step is done or there is no `loops.md` yet.
fn build_next_loop_prompt(path: &Path) -> String {
    let next_step = next_unchecked_loop_step(path);

    match next_step {
        Some(step) => format!(
            "You are working on this LoopDeck project. Use the `loopdeck-orchestrator` \
             skill conventions. Read `.loopdeck/loops.md` for full context. The next \
             unchecked step is: \"{step}\". Implement it. When done, update \
             `.loopdeck/loops.md` (mark the step `[x]`, refresh `## Current`) and \
             append any architectural decisions to `.loopdeck/decisions.md` per the \
             memory convention."
        ),
        None => String::from(
            "You are working on this LoopDeck project. Use the `loopdeck-orchestrator` \
             skill conventions. Review `.loopdeck/loops.md`, then propose and start \
             the next loop. When done, update `.loopdeck/loops.md` (refresh \
             `## Current`, add new steps under `## Next Steps`) and append any \
             architectural decisions to `.loopdeck/decisions.md` per the memory \
             convention.",
        ),
    }
}

/// Scan `.loopdeck/loops.md` for the first unchecked `- [ ]` step under
/// `## Next Steps`. Returns `None` if the file is missing, the section is
/// absent, or every step is already checked.
fn next_unchecked_loop_step(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path.join(".loopdeck").join("loops.md")).ok()?;

    // Walk to the `## Next Steps` section, then read lines until the next
    // `## ` heading. Return the first `- [ ]` item in that window.
    let mut in_next_steps = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_next_steps = trimmed.eq_ignore_ascii_case("## Next Steps");
            continue;
        }
        if in_next_steps {
            if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

/// Shared send pipeline used by `agent_start_loop` and `agent_send_message`.
///
/// Records the user turn to the transcript *before* sending (so a crash
/// mid-turn still captures intent), sends it, records the assistant turn, and
/// returns the structured response. The transcript append is best-effort: a
/// write failure is logged but doesn't fail the turn — the live result still
/// reaches the UI.
async fn send_and_record(
    state: &AppState,
    path: &Path,
    prompt: &str,
) -> Result<AgentResponse, AppError> {
    let session_arc = with_session(state, path).await?;
    let mut session = session_arc.lock().await;

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + receive.
    let response = session.send_message(prompt).await?;

    // 3. Record the assistant turn (best-effort). Done BEFORE the error check
    //    below so a failed turn (e.g. "Not logged in") still lands in the
    //    transcript — the user sees the error bubble AND its session_id is
    //    captured for a potential resume after they fix auth.
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
    );
    if let Err(e) = conversation::append_turn(path, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    // 4. Propagate claude-level errors as a real Err. claude doesn't crash on
    //    auth/config failures — it completes the stream with `is_error: true`
    //    and a human-readable `result` (e.g. "Not logged in · Please run
    //    /login"). Converting that to an AppError here makes the failure
    //    un-ignorable at the IPC boundary: every caller surfaces it instead of
    //    silently treating the turn as a success.
    if response.is_error {
        return Err(AppError::Agent(
            response.result.clone().trim().to_string(),
        ));
    }

    Ok(response)
}

/// Streaming variant of `send_and_record`.
///
/// Records the user turn, sends via `send_message_streaming` (which emits
/// per-block `ClaudeEvent`s on the channel as they arrive), then records the
/// assistant turn from the returned `AgentResponse`. The channel carries the
/// terminal `ClaudeEvent::Result` so the frontend gets the full aggregated
/// response inline — the return value here is just for transcript recording.
async fn send_and_record_streaming(
    state: &AppState,
    path: &Path,
    prompt: &str,
    channel: &Channel<ClaudeEvent>,
) -> Result<(), AppError> {
    let session_arc = with_session(state, path).await?;
    let mut session = session_arc.lock().await;

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + stream.
    let response = session.send_message_streaming(prompt, channel).await?;

    // 3. Record the assistant turn (best-effort).
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
    );
    if let Err(e) = conversation::append_turn(path, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_unchecked_step_finds_first_unchecked() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "# Loops\n\n## Current\n\n_No active loop._\n\n## Next Steps\n\
             - [x] Done thing\n\
             - [ ] First open step\n\
             - [ ] Second open step\n\n## History\n",
        )
        .unwrap();

        let step = next_unchecked_loop_step(&dir);
        assert_eq!(step.as_deref(), Some("First open step"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn next_unchecked_step_none_when_all_done() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "# Loops\n\n## Next Steps\n- [x] one\n- [x] two\n",
        )
        .unwrap();

        assert!(next_unchecked_loop_step(&dir).is_none());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn next_unchecked_step_none_when_no_file() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(next_unchecked_loop_step(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_prompt_uses_step_when_present() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(".loopdeck")).unwrap();
        std::fs::write(
            dir.join(".loopdeck").join("loops.md"),
            "## Next Steps\n- [ ] Wire up the thing\n",
        )
        .unwrap();

        let prompt = build_next_loop_prompt(&dir);
        assert!(prompt.contains("Wire up the thing"));
        assert!(prompt.contains("loopdeck-orchestrator"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn build_prompt_falls_back_when_no_step() {
        let dir = std::env::temp_dir().join(format!("loopdeck-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let prompt = build_next_loop_prompt(&dir);
        assert!(prompt.contains("propose and start"));
        assert!(!prompt.contains("next unchecked step is"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
