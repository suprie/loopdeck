use crate::agents::AgentResponse;
use crate::agents::ClaudeEvent;
use crate::claude_session::{
    ClaudeSession, InterruptSlot, PermissionSlot, QuestionAnswers, QuestionSlot,
};
use crate::config::{self, AgentConfig, GlobalConfig, ProjectEntry, ProjectStatus, RunState};
use crate::conversation::{self, ConversationSummary, ConversationTurn};
use crate::error::AppError;
use crate::git;
use crate::memory::{self, Decision, LoopStatus};
use crate::permission::Decision as PermissionDecision;
use crate::permission::PermissionPolicy;
use crate::project::{self, ProjectMeta};
use crate::scanner::{self, DiscoveredRepo};
use chrono::Utc;
use serde::Deserialize;
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
    /// Per-project pending `AskUserQuestion` slots. When Claude asks a
    /// question mid-turn, the read loop parks on the slot's oneshot receiver;
    /// the `agent_answer_question` command pops the sender here to deliver the
    /// user's answers and wake the parked turn. Keyed by project path so
    /// different projects' questions can't collide.
    pub pending_answers: Mutex<HashMap<PathBuf, QuestionSlot>>,
    /// Per-project pending manual-approval slots for `can_use_tool` requests on
    /// mutating/executing tools (Bash/Edit/Write/…). When such a tool call
    /// arrives, the read loop parks on the slot's oneshot receiver; the
    /// `agent_answer_permission` command pops the sender here to deliver the
    /// user's Allow/Deny and wake the parked turn. Mirrors `pending_answers`.
    pub pending_permissions: Mutex<HashMap<PathBuf, PermissionSlot>>,
    /// Per-project interrupt slots for graceful Stop. The streaming read loop
    /// installs a fresh oneshot sender per turn and `select!`s on the
    /// receiver; `agent_interrupt` pops + fires the sender, the loop wakes and
    /// writes the `interrupt` control_request, ending the turn while keeping
    /// the live process (and its context) alive.
    pub interrupt_slots: Mutex<HashMap<PathBuf, InterruptSlot>>,
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
        uncommitted: git_info.uncommitted.into(),
        run_state: RunState::Idle,
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
        let fresh_uncommitted: config::UncommittedStats = git_info.uncommitted.into();
        if entry.uncommitted != fresh_uncommitted {
            entry.uncommitted = fresh_uncommitted;
            changed = true;
        }

        entry.current_loop = project::read_current_loop(entry.path.as_path());
    }

    if changed {
        config.save()?;
    }

    // Derive ephemeral run_state per project from live session + pending slots.
    // Done after the save so transient state never reaches disk.
    let mut out = config.projects.clone();
    derive_run_states(&state, &mut out);

    Ok(out)
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

/// Resolve and validate a user-supplied path before handing it to an OS
/// opener (Finder/Terminal/explorer). The path originates from a Tauri IPC
/// argument, so although it is normally a legitimate repo path we treat it as
/// untrusted: this canonicalizes it and rejects anything that doesn't resolve
/// to an existing directory. That blocks scheme-handler tricks (e.g. macOS
/// `open "x-apple-..."`) and shell-metachar injection downstream.
fn resolve_dir_arg(path: &str) -> Result<PathBuf, AppError> {
    let resolved = std::fs::canonicalize(path)
        .map_err(|_| AppError::Scan(format!("Path does not exist or is not accessible: {path}")))?;
    if !resolved.is_dir() {
        return Err(AppError::Scan(format!("Not a directory: {path}")));
    }
    Ok(resolved)
}

/// Open the repository path in the system file manager (Finder on macOS).
#[tauri::command]
pub async fn open_in_finder(path: String) -> Result<(), AppError> {
    debug!("open_in_finder called for path: {path}");
    // Validate before any opener sees it. `open`, `xdg-open`, and `explorer`
    // otherwise interpret some strings as URLs / handlers.
    let resolved = resolve_dir_arg(&path)?;

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
pub async fn open_in_terminal(path: String) -> Result<(), AppError> {
    debug!("open_in_terminal called for path: {path}");
    let resolved = resolve_dir_arg(&path)?;
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
            if let Ok(mut child) = std::process::Command::new(term)
                .arg("--working-directory")
                .arg(&path_str)
                .spawn()
            {
                // Don't wait — let the terminal run independently.
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
    entry.uncommitted = git_info.uncommitted.into();

    config::update_project_status(entry);

    let mut result = entry.clone();
    config.save()?;

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
/// sends it through the **fresh-start** pipeline: any live session is dropped,
/// the transcript is archived, and a brand-new claude process is spawned
/// **without** `--resume` — Start always begins a new conversation.
///
/// Concurrency: uses `try_lock`. If a turn is already in flight on this
/// project, Start is rejected immediately with "agent is busy" rather than
/// queueing (only `agent_send_message` queues). The successful `try_lock` is
/// the proof that the prior session is idle and safe to replace.
#[tauri::command]
pub async fn agent_start_loop(
    path: String,
    state: State<'_, AppState>,
) -> Result<AgentResponse, AppError> {
    debug!("agent_start_loop called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let prompt = build_next_loop_prompt(&repo_path);
    let response = start_fresh_and_record(&state, &repo_path, &prompt).await?;
    info!("agent_start_loop complete for: {path}");
    Ok(response)
}

/// Send a free-form follow-up message to the project's agent session.
///
/// Continues the **existing** conversation: reuses the live process if present,
/// or (after an app restart, when no live process exists) re-spawns claude with
/// `--resume <last_session_id>` so the model's context is restored. Both turns
/// are recorded. Contrast with `agent_start_loop`, which always begins a fresh
/// conversation.
///
/// Concurrency: uses `lock().await`, so a follow-up sent while a turn is in
/// flight on this project queues behind it (different projects run in parallel).
#[tauri::command]
pub async fn agent_send_message(
    path: String,
    prompt: String,
    state: State<'_, AppState>,
) -> Result<AgentResponse, AppError> {
    debug!("agent_send_message called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
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
/// Fresh-start semantics, identical to `agent_start_loop`: drops any live
/// session, archives the transcript, spawns without `--resume`, rejects with
/// "agent is busy" if a turn is in flight (`try_lock`).
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
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    let prompt = build_next_loop_prompt(&repo_path);
    start_fresh_and_record_streaming(&state, &repo_path, &prompt, &on_event).await?;
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
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
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
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }
    Ok(conversation::load_conversation(&repo_path))
}

/// List all conversations (active + archived) for the history UI.
///
/// Returns one `ConversationSummary` per transcript file in the sessions dir,
/// sorted newest-first by last-turn timestamp. Each row carries an id
/// (`"active"` or an archive stem) the frontend passes back to
/// `agent_get_conversation_by_id` to load the turns.
#[tauri::command]
pub async fn agent_list_conversations(
    path: String,
    _state: State<'_, AppState>,
) -> Result<Vec<ConversationSummary>, AppError> {
    debug!("agent_list_conversations called for path: {path}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }
    Ok(conversation::list_conversations(&repo_path))
}

/// Load a specific conversation by id (`"active"` or an archive stem).
///
/// Used by the history viewer to open a past conversation read-only. Returns
/// an empty vec for an unknown id (e.g. an archive deleted out of band) — the
/// UI shows an empty state rather than erroring.
#[tauri::command]
pub async fn agent_get_conversation_by_id(
    path: String,
    id: String,
    _state: State<'_, AppState>,
) -> Result<Vec<ConversationTurn>, AppError> {
    debug!("agent_get_conversation_by_id called for path: {path}, id: {id}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }
    Ok(conversation::load_conversation_by_id(&repo_path, &id))
}

/// Promote an archived conversation to active, returning its `session_id`.
///
/// Called by the frontend when the user sends a follow-up while viewing an
/// archived conversation. The backend:
/// 1. `promote_to_active` — rotates the current `active.jsonl` aside (so it
///    survives in history) and seeds a fresh active transcript with the chosen
///    archive's turns.
/// 2. Extracts the most recent assistant `session_id` from that conversation
///    so the agent pipeline can `--resume` it, restoring the model's context.
///
/// Returns `None` when the source has no `session_id` (empty, or only user
/// turns) — in that case the frontend proceeds with a non-resume start. The
/// live session is also dropped (its context is now stale relative to the
/// promoted transcript); the next send re-spawns with the returned resume id.
#[tauri::command]
pub async fn agent_promote_to_active(
    path: String,
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, AppError> {
    debug!("agent_promote_to_active called for path: {path}, id: {id}");
    let repo_path = PathBuf::from(&path);
    if !repo_path.exists() {
        return Err(AppError::ProjectNotFound(format!(
            "Path does not exist: {path}"
        )));
    }

    // Extract the resume id BEFORE promoting (after promotion the turns live
    // in active.jsonl, but reading from the source id is unambiguous).
    let resume_id = conversation::session_id_for_conversation(&repo_path, &id);

    // Promote: archive current active, seed new active from the source. No-op
    // for `id == "active"` or an unknown/empty source — both safe here.
    conversation::promote_to_active(&repo_path, &id)?;

    // Drop any live session — its in-process context is now stale relative to
    // the promoted transcript. The next send re-spawns with `--resume <id>`
    // via `with_session` (which reads `last_session_id` off the new active).
    let removed = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(&repo_path)
        .is_some();
    if removed {
        debug!("dropped live session for promote of {id} in: {path}");
    }

    info!("agent_promote_to_active complete for: {path}, id: {id}");
    Ok(resume_id)
}

/// Reset the project's agent session: drop the live process and archive the
/// transcript.
///
/// The next `agent_start_loop` starts a fresh conversation (no `--resume`).
/// The archived transcript is preserved as `archive-<ts>.jsonl` for history.
#[tauri::command]
pub async fn agent_reset_session(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
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

/// Wire shape of a single answer as sent by the frontend
/// (`agent_answer_question`'s `answers` map value).
///
/// `labels` carries the selected option label(s); `other_text` carries the
/// free-text "Other…" value when the user typed one instead of (or alongside)
/// picking a canned option. Both optional so the frontend can send whichever
/// applies.
#[derive(Debug, Deserialize)]
pub struct AnswerWire {
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub other_text: Option<String>,
}

/// Answer a pending `AskUserQuestion` for the given project.
///
/// Called by the frontend when the user submits the question card. Pops the
/// oneshot sender from the per-project `pending_answers` slot and sends the
/// answers — this wakes the read loop (parked in
/// `ClaudeSession::answer_ask_user_question`), which writes the
/// `control_response` with `updatedInput.answers` and the turn resumes.
///
/// Returns an error if no question is pending for this project (the user
/// submitted without a prompt, or the turn already ended/timed out).
#[tauri::command]
pub async fn agent_answer_question(
    path: String,
    request_id: String,
    answers: HashMap<String, AnswerWire>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!(
        "agent_answer_question called for path: {path}, request_id: {request_id}, {} answers",
        answers.len()
    );
    let repo_path = PathBuf::from(&path);

    // Pop the sender for this project. There's at most one pending question at
    // a time (Claude blocks on each), so this is a single take of the sender
    // field — the slot entry is cleared entirely so `agent_pending_question`
    // stops reporting it as pending.
    let sender = {
        let guard = state
            .pending_answers
            .lock()
            .map_err(|_| AppError::LockError)?;
        guard
            .get(&repo_path)
            .and_then(|slot| {
                slot.lock()
                    .ok()
                    .and_then(|mut g| g.take())
            })
            .and_then(|pending| pending.sender)
            .ok_or_else(|| {
                AppError::Agent(
                    "no pending question for this project (it may have timed out or already been answered)".into(),
                )
            })?
    };

    // Convert the wire answers into the backend type and send. The sender
    // drops on send, so the slot is now empty for the next question.
    let mapped: QuestionAnswers = answers
        .into_iter()
        .map(|(q, a)| {
            (
                q,
                crate::claude_session::QuestionAnswer {
                    labels: a.labels,
                    other_text: a.other_text.filter(|t| !t.trim().is_empty()),
                },
            )
        })
        .collect();

    sender.send(mapped).map_err(|_| {
        AppError::Agent(
            "the pending question is no longer waiting for an answer (turn ended)".into(),
        )
    })?;

    info!("agent_answer_question delivered for: {path}");
    Ok(())
}

/// Wire shape of the user's manual-approval verdict, as sent by the frontend
/// (`agent_answer_permission`'s `decision` arg).
///
/// `allow: true` → run the tool; `allow: false` → deny it. `reason` is
/// optional and only meaningful on a deny (it's surfaced to the model as the
/// deny message). Mirrors `AnswerWire`'s role for `agent_answer_question`.
#[derive(Debug, Deserialize)]
pub struct ApprovalWire {
    pub allow: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Resolve a pending manual-approval request for the given project.
///
/// Called by the frontend when the user clicks Allow or Deny on the approval
/// card. Pops the oneshot sender from the per-project `pending_permissions`
/// slot and sends the `Decision` — this wakes the read loop (parked in
/// `ClaudeSession::answer_manual_permission`), which writes the matching
/// `control_response` and the turn resumes (or, on deny, recovers).
///
/// Returns an error if no approval is pending for this project (the user
/// clicked after the turn ended/timed out, or there was never a prompt).
#[tauri::command]
pub async fn agent_answer_permission(
    path: String,
    request_id: String,
    decision: ApprovalWire,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    debug!(
        "agent_answer_permission called for path: {path}, request_id: {request_id}, allow: {}",
        decision.allow
    );
    let repo_path = PathBuf::from(&path);

    // Pop the sender for this project. At most one pending approval at a time
    // (Claude blocks on each control_request), so this is a single take of the
    // sender field — the slot entry is cleared entirely so
    // `agent_pending_permission` stops reporting it as pending.
    let sender = {
        let guard = state
            .pending_permissions
            .lock()
            .map_err(|_| AppError::LockError)?;
        guard
            .get(&repo_path)
            .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()))
            .and_then(|pending| pending.sender)
            .ok_or_else(|| {
                AppError::Agent(
                    "no pending permission approval for this project (it may have timed out or already been answered)".into(),
                )
            })?
    };

    // Convert the wire verdict into the policy's `Decision` vocabulary — the
    // single source of truth the read loop writes back. A deny with no reason
    // gets a generic message so the model always sees *something*.
    let verdict = if decision.allow {
        PermissionDecision::Allow
    } else {
        PermissionDecision::Deny(
            decision
                .reason
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| String::from("denied by user")),
        )
    };

    sender.send(verdict).map_err(|_| {
        AppError::Agent(
            "the pending approval is no longer waiting for an answer (turn ended)".into(),
        )
    })?;

    info!("agent_answer_permission delivered for: {path}");
    Ok(())
}

/// Gracefully interrupt the in-flight turn for a project.
///
/// Called by the frontend's Stop button. Pops the oneshot sender from the
/// per-project `interrupt_slots` and fires it; the streaming read loop (which
/// `select!`s on the receiver) wakes, writes the graceful `interrupt`
/// control_request to the live process, and ends the turn. The live process
/// and its conversation context survive (unlike `agent_reset_session`, which
/// kills both) — the next send resumes the same conversation.
///
/// Returns `Ok(())` whether or not a turn was in flight: no-op when idle is
/// the friendlier contract for a UI button (the user just sees "stopped").
/// Returns an error only on state corruption (lock poisoned).
///
/// **Limitation:** if the turn is currently parked on an AskUserQuestion or
/// manual-approval card (off `read_line`), the interrupt isn't observed this
/// turn — the user should dismiss the card instead. The next turn picks up
/// the interrupt slot fresh.
#[tauri::command]
pub async fn agent_interrupt(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    debug!("agent_interrupt called for path: {path}");
    let repo_path = PathBuf::from(&path);

    // Pop + fire the sender. There's at most one per project (cleared at turn
    // end), so this is a single take. `send` failing means the receiver was
    // already dropped (turn ended between the UI click and here) — treat as a
    // no-op success so the button feels responsive either way.
    let fired = {
        let guard = state
            .interrupt_slots
            .lock()
            .map_err(|_| AppError::LockError)?;
        guard
            .get(&repo_path)
            .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()))
            .map(|sender| sender.send(()).is_ok())
    };

    if !fired.unwrap_or(false) {
        debug!("agent_interrupt: no in-flight turn for {path} (no-op)");
    } else {
        info!("agent_interrupt fired for: {path}");
    }
    Ok(())
}

// ── Agent session helpers ──────────────────────────────────────────────────
//
// Two pipelines own session lifecycle:
// - `with_session` (below) — get-or-spawn + `lock().await`. Used by
//   `agent_send_message`: reuses the live process, or `--resume`s after a
//   restart. Queues behind a running turn.
// - `spawn_fresh_locked` (further below) — force-spawn + `try_lock`. Used by
//   `agent_start_loop[_streaming]`: always a fresh conversation, rejects when
//   busy.
//
// Both uphold the per-project turn-lock invariant (one stdin, one process):
// same-project turns never run concurrently, different projects run in parallel.

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
    let mut map_guard = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?;

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
    let session = ClaudeSession::spawn(
        &path.to_path_buf(),
        &agent_config,
        resume_id.as_deref(),
        PermissionPolicy::allow_by_default(),
    )?;
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

/// Get (or create) the per-project `AskUserQuestion` slot.
///
/// One slot per project path, shared (via `Arc`) between the read loop (which
/// stores the oneshot sender when Claude asks a question) and the
/// `agent_answer_question` command (which pops the sender to deliver answers).
/// The slot persists across turns so it doesn't need re-creating each time;
/// its contents are always `None` outside a pending question.
fn question_slot(state: &AppState, path: &Path) -> Result<QuestionSlot, AppError> {
    let mut guard = state
        .pending_answers
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone())
}

/// Get (or create) the per-project manual-approval slot.
///
/// The permission counterpart of `question_slot`: one `PermissionSlot` per
/// project path, shared between the read loop (stores the oneshot sender when
/// a mutating tool needs approval) and the `agent_answer_permission` command
/// (pops the sender to deliver the verdict). Persistent across turns; always
/// `None` outside a pending approval.
fn permission_slot(state: &AppState, path: &Path) -> Result<PermissionSlot, AppError> {
    let mut guard = state
        .pending_permissions
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone())
}

/// Get (or create) the per-project graceful-interrupt slot.
///
/// The streaming read loop installs a oneshot sender here at turn start and
/// clears it at turn end; `agent_interrupt` pops + fires the sender to end the
/// turn gracefully. Persistent across turns; always `None` outside a running
/// turn.
fn interrupt_slot(state: &AppState, path: &Path) -> Result<InterruptSlot, AppError> {
    let mut guard = state
        .interrupt_slots
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard
        .entry(path.to_path_buf())
        .or_insert_with(|| Arc::new(Mutex::new(None)))
        .clone())
}

/// Derive the ephemeral `RunState` for each project from live `AppState`.
///
/// Strict live state (no recency heuristic):
/// - `Waiting` — there is a pending manual-approval (`pending_permissions`) or
///   a pending `AskUserQuestion` (`pending_answers`) for the project path. The
///   in-flight turn is parked awaiting the user.
/// - `Working` — a `claude_sessions` entry exists for the path AND its inner
///   `tokio::Mutex` is currently held (a turn is streaming). We check by
///   `try_lock`; success ⇒ idle, failure ⇒ busy.
/// - `Idle` — no live session, or session exists but no turn is in flight.
///
/// Note: `Done` is intentionally never set here. We don't track "last turn
/// finished at" timestamps globally; the frontend derives its own transient
/// "done" affordance from streaming-result events if desired.
fn derive_run_states(state: &AppState, projects: &mut [ProjectEntry]) {
    // Snapshot the pending-slot keys without holding the locks across the
    // (potentially async-blocking) inner session `try_lock`.
    let pending_paths: std::collections::HashSet<PathBuf> = {
        let perms = state
            .pending_permissions
            .lock()
            .map_err(|_| AppError::LockError);
        let answers = state
            .pending_answers
            .lock()
            .map_err(|_| AppError::LockError);
        let (Ok(perms), Ok(answers)) = (perms, answers) else {
            // If we can't observe pending state, leave everything as-is.
            return;
        };
        perms.keys().chain(answers.keys()).cloned().collect()
    };

    for entry in projects.iter_mut() {
        if pending_paths.contains(&entry.path) {
            entry.run_state = RunState::Waiting;
            continue;
        }
        entry.run_state = if project_busy(state, &entry.path) {
            RunState::Working
        } else {
            RunState::Idle
        };
    }
}

/**
 * Is a streaming agent turn currently in flight for `path`?
 *
 * True when a `claude_sessions` entry exists AND its inner `tokio::Mutex` is
 * held (a turn is streaming). Checked via non-blocking `try_lock`. Used both by
 * `derive_run_states` (for `list_projects`/`rescan_project`) and by the
 * `agent_is_busy` command (for frontend reconciliation after navigation).
 *
 * Outer-map guards are held only over the synchronous Arc lookup, never across
 * an `.await`.
 */
fn project_busy(state: &AppState, path: &Path) -> bool {
    let session_arc = state
        .claude_sessions
        .lock()
        .ok()
        .and_then(|m| m.get(path).cloned());
    match session_arc {
        Some(arc) => arc.try_lock().is_err(),
        None => false,
    }
}

/// Report whether an agent turn is currently in flight for a project.
///
/// Used by the frontend to reconcile `busy` state after navigating away and
/// back to the Agent page mid-turn. Without it, the unmounted AgentPanel loses
/// the streaming thread entirely; this command lets a freshly-mounted panel
/// honestly report "still working" and poll the transcript until it lands.
#[tauri::command]
pub async fn agent_is_busy(path: String, state: State<'_, AppState>) -> Result<bool, AppError> {
    Ok(project_busy(&state, &PathBuf::from(&path)))
}

// ── Pending-interaction payloads (reconciliation after navigation) ───────────
//
// When an AskUserQuestion or manual-approval request parks the turn, the
// request payload now lives in `AppState.pending_*` alongside the oneshot
// sender (see `PendingQuestion` / `PendingPermission`). These commands expose
// the payload without consuming the sender, so a freshly-mounted AgentPanel —
// whose predecessor's Tauri Channel (and thus the original parking event) was
// lost on unmount — can re-materialize the card.

/// Serializable payload for a pending manual-approval request.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPermissionInfo {
    pub request_id: String,
    pub tool_name: String,
    pub input: String,
}

/// Serializable payload for a pending AskUserQuestion request.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingQuestionInfo {
    pub request_id: String,
    pub questions: Vec<crate::agents::AskUserQuestionSpec>,
}

/// Read the pending manual-approval payload for a project, if any.
///
/// Does NOT consume the sender — only the payload. The frontend uses this to
/// re-render the Allow/Deny card after navigating away and back. Returns `None`
/// when no approval is pending.
#[tauri::command]
pub async fn agent_pending_permission(
    path: String,
    state: State<'_, AppState>,
) -> Result<Option<PendingPermissionInfo>, AppError> {
    let repo_path = PathBuf::from(&path);
    let guard = state
        .pending_permissions
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard.get(&repo_path).and_then(|slot| {
        slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingPermissionInfo {
                request_id: p.request_id.clone(),
                tool_name: p.tool_name.clone(),
                input: p.input.clone(),
            })
        })
    }))
}

/// Read the pending AskUserQuestion payload for a project, if any.
///
/// Does NOT consume the sender — only the payload. The frontend uses this to
/// re-render the question card after navigating away and back. Returns `None`
/// when no question is pending.
#[tauri::command]
pub async fn agent_pending_question(
    path: String,
    state: State<'_, AppState>,
) -> Result<Option<PendingQuestionInfo>, AppError> {
    let repo_path = PathBuf::from(&path);
    let guard = state
        .pending_answers
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard.get(&repo_path).and_then(|slot| {
        slot.lock().ok().and_then(|g| {
            g.as_ref().map(|p| PendingQuestionInfo {
                request_id: p.request_id.clone(),
                questions: p.questions.clone(),
            })
        })
    }))
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
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + receive.
    let response = session.send_message(prompt, &qslot, &pslot, &islot).await?;

    // 3. Record the assistant turn (best-effort). Done BEFORE the error check
    //    below so a failed turn (e.g. "Not logged in") still lands in the
    //    transcript — the user sees the error bubble AND its session_id is
    //    captured for a potential resume after they fix auth. Includes the
    //    model's thinking chain and tool calls so the transcript records how
    //    the answer was reached, not just the final summary.
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
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
        return Err(AppError::Agent(response.result.clone().trim().to_string()));
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
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + stream.
    let response = session
        .send_message_streaming(prompt, channel, &qslot, &pslot, &islot)
        .await?;

    // 3. Record the assistant turn (best-effort). Includes thinking + tool
    //    calls so the persisted transcript captures the full reasoning trail,
    //    not just the final summary text.
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
    );
    if let Err(e) = conversation::append_turn(path, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    Ok(())
}

// ── Fresh-start pipeline (agent_start_loop[_streaming]) ────────────────────
//
// Start always begins a NEW conversation: drop any live session, archive the
// transcript, spawn a fresh claude process WITHOUT `--resume`. This is the
// deliberate contrast with `with_session` (used by `agent_send_message`),
// which reuses the live process or `--resume`s after a restart.
//
// Concurrency: Start uses `try_lock` — if a turn is already in flight on this
// project, the start is rejected immediately ("agent is busy") instead of
// queueing. The successful `try_lock` is also the proof that the prior session
// is idle and therefore safe to drop and replace mid-map. Send, by contrast,
// uses `lock().await` and queues.

/// Force-spawn a fresh `ClaudeSession` for `path`, replacing any live one.
///
/// 1. `try_lock` the existing arc (if any). `Err` ⇒ a turn is in flight on the
///    current session → reject with "agent is busy" (Start must not interrupt a
///    running turn). `Ok` ⇒ the old session is provably idle.
/// 2. Drop the old arc from the map (last reference drops → `Drop` closes stdin
///    → claude exits → child reaped).
/// 3. `archive_conversation` — rotate `active.jsonl` aside so the new
///    conversation begins fresh.
/// 4. Spawn a NEW `ClaudeSession` with `resume_session_id = None` (never resume
///    on Start), insert its arc into the map, and return the arc.
///
/// Returns an owned `Arc` (mirroring `with_session`'s contract) so the caller
/// `.lock().await`s it. The busy-check `try_lock` in phase 1 only proves the
/// *old* session was idle; between then and the caller taking the new arc's
/// lock, a concurrent producer could in principle race — but the only producers
/// are Start/Send, and the frontend disables both while a turn is in flight, so
/// the race window is closed in practice for this single-user app.
async fn spawn_fresh(
    state: &AppState,
    path: &Path,
) -> Result<Arc<tokio::sync::Mutex<ClaudeSession>>, AppError> {
    // ── Phase 1: try_lock the existing arc to prove it's idle. ──
    // Scoped so the map's std Mutex guard is dropped before we await anything.
    {
        let map_guard = state
            .claude_sessions
            .lock()
            .map_err(|_| AppError::LockError)?;
        if let Some(arc) = map_guard.get(path) {
            // Try to acquire the per-project turn lock non-blockingly. Holding
            // the std Mutex here is fine — try_lock is synchronous, no .await.
            if arc.try_lock().is_err() {
                return Err(AppError::Agent(
                    "agent is busy — wait for the current turn to finish before starting a new conversation".into(),
                ));
            }
            // try_lock succeeded ⇒ idle. The guard drops here; we proceed to
            // replace the session below.
        }
    }

    // ── Phase 2: drop the old arc from the map (reaps the child via Drop). ──
    let dropped = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(path);
    if dropped.is_some() {
        debug!(
            "dropped live claude session for fresh start: {}",
            path.display()
        );
    }

    // ── Phase 3: archive the transcript (rotate active.jsonl aside). ──
    // Surfaced as a real error: the user asked for a fresh conversation and
    // didn't get one.
    conversation::archive_conversation(path)?;

    // ── Phase 4: spawn fresh (no --resume) and insert. ──
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

    let session = ClaudeSession::spawn(
        &path.to_path_buf(),
        &agent_config,
        None,
        PermissionPolicy::allow_by_default(),
    )?;
    let arc = Arc::new(tokio::sync::Mutex::new(session));
    state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .insert(path.to_path_buf(), Arc::clone(&arc));

    Ok(arc)
}

/// Fresh-start send pipeline used by `agent_start_loop`. Mirrors
/// `send_and_record` but spawns a brand-new session (no `--resume`) and rejects
/// when busy instead of queueing. See `spawn_fresh` for the lifecycle.
async fn start_fresh_and_record(
    state: &AppState,
    path: &Path,
    prompt: &str,
) -> Result<AgentResponse, AppError> {
    let session_arc = spawn_fresh(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    //    Marked `user_loop` — this prompt was auto-built from
    //    `.loopdeck/loops.md` by `build_next_loop_prompt`, not typed by the
    //    human. The UI renders these as compact system rows instead of
    //    user chat bubbles so they don't drown out real messages.
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user_loop(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + receive.
    let response = session.send_message(prompt, &qslot, &pslot, &islot).await?;

    // 3. Record the assistant turn (best-effort, includes thinking + tool calls).
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
    );
    if let Err(e) = conversation::append_turn(path, &assistant_turn) {
        tracing::warn!("failed to append assistant turn to transcript: {e}");
    }

    // 4. Propagate claude-level errors (is_error: true) as a real Err so every
    //    caller surfaces them instead of silently treating the turn as success.
    if response.is_error {
        return Err(AppError::Agent(response.result.clone().trim().to_string()));
    }

    Ok(response)
}

/// Streaming variant of `start_fresh_and_record`, used by
/// `agent_start_loop_streaming`. Same fresh-start + reject-when-busy semantics.
async fn start_fresh_and_record_streaming(
    state: &AppState,
    path: &Path,
    prompt: &str,
    channel: &Channel<ClaudeEvent>,
) -> Result<(), AppError> {
    let session_arc = spawn_fresh(state, path).await?;
    let mut session = session_arc.lock().await;
    let qslot = question_slot(state, path)?;
    let pslot = permission_slot(state, path)?;
    let islot = interrupt_slot(state, path)?;

    // 1. Record the user turn first (crash-safety: intent survives).
    //    Marked `user_loop` (see `start_fresh_and_record` for rationale).
    if let Err(e) = conversation::append_turn(path, &ConversationTurn::user_loop(prompt)) {
        tracing::warn!("failed to append user turn to transcript: {e}");
    }

    // 2. Send + stream.
    let response = session
        .send_message_streaming(prompt, channel, &qslot, &pslot, &islot)
        .await?;

    // 3. Record the assistant turn (best-effort, includes thinking + tool calls).
    let assistant_turn = ConversationTurn::assistant(
        response.result.clone(),
        response.session_id.clone(),
        response.is_error,
        response.usage.clone(),
        response.duration_ms,
        response.thinking.clone(),
        response.tool_calls.clone(),
        response.blocks.clone(),
        response.tasks.clone(),
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
