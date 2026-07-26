//! Shared `AppState`, the IPC-facing entry types, and the plumbing shared
//! across every command module.
//!
//! Everything here is `pub(crate)` so the sibling command modules
//! (`composer`, `project`, `config_cmds`, `epics`, `agent`) can reach it via
//! `use super::state::*`, and the top-level `commands::mod.rs` re-exports the
//! public surface (`AppState`, `DirEntry`, `SkillEntry`) for `lib.rs`.
//!
//! # Lock invariant
//! `claude_sessions` uses a two-layer lock so projects run concurrently while
//! turns within one project serialize (one process, one stdin):
//! - **Outer** `std::sync::Mutex` guards the map for insert/lookup only —
//!   held for microseconds, NEVER across `.await` (would deadlock / is unsound
//!   across threads). The guard is dropped before any async work.
//! - **Inner** `tokio::sync::Mutex` per project, held for one full turn
//!   (seconds–minutes). Different projects take different inner locks, so
//!   they run in true parallel.

use crate::claude_session::{InterruptSlot, PermissionSlot, QuestionSlot};
use crate::config::{AgentConfig, GlobalConfig, ProjectEntry, RunState};
use crate::conversation;
use crate::error::AppError;
use crate::harness::HarnessSession;
use crate::paths;
use crate::permission::PermissionPolicy;
use crate::secrets;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Shared application state managed by Tauri.
///
/// See the [module docs](self) for the two-layer lock invariant on
/// `claude_sessions`.
pub struct AppState {
    pub config: Mutex<GlobalConfig>,
    pub claude_sessions: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<HarnessSession>>>>,
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

/// A single child entry of a project directory, for the chat composer's
/// `@`-mention file/folder autocomplete. `path` is project-relative (forward
/// slashes) so the frontend can insert it verbatim as `@<path>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    /// Entry basename (e.g. `Chat.tsx`).
    pub name: String,
    /// Whether the entry is a directory.
    pub is_dir: bool,
    /// Project-relative path (forward slashes), e.g. `src/components/detail/Chat.tsx`.
    pub path: String,
}

/// A skill installed for a project, surfaced by the composer's `/`-skill
/// discovery menu. Read from `<repo>/.claude/skills/<dir>/SKILL.md` — the files
/// `copy_skills()` writes during project bootstrap.
///
/// `name` is the SKILL.md frontmatter `name` (e.g. `loopdeck:rust-expert`),
/// which is what the `claude` CLI invokes the skill by — so the frontend
/// inserts it verbatim as `/<name>`. It is distinct from `directory`, the
/// on-disk folder name (`loopdeck-rust-expert`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    /// Frontmatter `name` — the invocation token the `claude` CLI recognizes.
    pub name: String,
    /// On-disk skill directory name (e.g. `loopdeck-rust-expert`).
    pub directory: String,
    /// Frontmatter `description`, used by the menu to show what each skill does.
    /// Empty string if the SKILL.md has no `description:` field.
    pub description: String,
    /// Frontmatter `argument-hint`, shown as a dimmed placeholder next to the
    /// skill name (e.g. `<prd-file-path>`) to cue the user what to type after.
    /// Empty string when the skill takes no arguments.
    #[serde(default)]
    pub argument_hint: String,
}

/// Map a `spawn_blocking` join failure (the task panicked or was cancelled) to
/// an `AppError`.
///
/// The blocking tasks we offload (recursive `walkdir`, per-repo `git`
/// subprocesses, file reads) don't panic on their own, but the join error
/// surface must still be converted to `AppError` so it can cross the Tauri IPC
/// boundary instead of leaking a raw `tokio::task::JoinError`.
pub(crate) fn blocking_task_failed(e: tokio::task::JoinError) -> AppError {
    AppError::BlockingTask(format!("background task failed: {e}"))
}

/// Resolve `path` to the canonical, **registered** project root.
///
/// Convenience for project-scoped commands that read project state and don't
/// otherwise need to hold the config lock: briefly locks, resolves via the
/// shared boundary helper ([`paths::resolve_registered_root`]), and returns
/// the canonical root. Commands that need to mutate the registry under a lock
/// resolve inline instead so the lock spans the mutation.
pub(crate) fn resolve_root(state: &AppState, path: &str) -> Result<PathBuf, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    paths::resolve_registered_root(&config, path)
}

/// Read the agent config from the registry and inject the auth token from the
/// local secrets file.
///
/// The token is never stored in `config.yaml` (it lives in a separate
/// owner-only file — see `secrets`), so it must be resolved here, at spawn
/// time. The returned value is a local owned `AgentConfig` passed by reference
/// to `ClaudeSession::spawn`, which sets it as a child env var
/// (`ANTHROPIC_AUTH_TOKEN`) and then drops it — the plaintext token is never
/// held on the long-lived `Mutex<GlobalConfig>`.
///
/// A missing secrets-file token resolves to `None`, preserving the prior
/// behaviour where a user may rely on `ANTHROPIC_AUTH_TOKEN` inherited from
/// their shell.
pub(crate) fn resolve_agent_config(state: &AppState) -> Result<AgentConfig, AppError> {
    let mut agent_config = state
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
    agent_config.auth_token = secrets::load_auth_token()?;
    Ok(agent_config)
}

/// Resolve the per-project `PermissionPolicy` from the registry.
///
/// Looks up the project's `autonomous` flag under the config lock and returns
/// `Autonomous` (skip the manual-approval card) or `ConfirmChanges` (the
/// default). The destructive floor applies under both — it runs in `decide()`
/// before the mode match, so `rm -rf` / force-push / `curl|sh` / `sudo` are
/// denied regardless. An unregistered path resolves to `ConfirmChanges`
/// (safest), so a stale registry entry can never silently grant autonomy.
pub(crate) fn resolve_permission_policy(state: &AppState, path: &Path) -> PermissionPolicy {
    let autonomous = state
        .config
        .lock()
        .map(|cfg| {
            cfg.projects
                .iter()
                .find(|p| p.path.as_path() == path)
                .map(|p| p.autonomous)
                .unwrap_or(false)
        })
        .unwrap_or(false);
    if autonomous {
        PermissionPolicy::with_mode(crate::permission::PermissionMode::Autonomous)
    } else {
        PermissionPolicy::confirm_changes()
    }
}

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
pub(crate) async fn with_session(
    state: &AppState,
    path: &Path,
) -> Result<Arc<tokio::sync::Mutex<HarnessSession>>, AppError> {
    // Resolve before taking the session-map lock so lock ordering remains
    // config → sessions everywhere (project listing uses the same order).
    let agent_config = resolve_agent_config(state)?;
    let desired_harness = agent_config.harness;

    // ── Outer (map) lock: held only to read/insert the Arc. ──
    let mut map_guard = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?;

    if let Some(arc) = map_guard.get(path).cloned() {
        // Apply a changed harness as soon as the existing session is idle. If
        // a turn is still running, preserve the live process; the next send
        // after it completes will replace it.
        if let Ok(session) = arc.try_lock() {
            if session.harness() == desired_harness {
                return Ok(Arc::clone(&arc));
            }
            drop(session);
            map_guard.remove(path);
        } else {
            return Ok(Arc::clone(&arc));
        }
    }

    // No compatible live session — spawn one. The config was resolved before
    // the map lock to avoid nested lock-order inversions.
    let policy = resolve_permission_policy(state, path);

    let resume_id = conversation::last_session_id(path);
    let session = HarnessSession::spawn(path, &agent_config, resume_id.as_deref(), policy)?;
    let arc = Arc::new(tokio::sync::Mutex::new(session));
    map_guard.insert(path.to_path_buf(), Arc::clone(&arc));
    // ── map_guard (std Mutex) dropped here as this scope ends. ──
    Ok(arc)
}

/// Get (or create) the per-project `AskUserQuestion` slot.
///
/// One slot per project path, shared (via `Arc`) between the read loop (which
/// stores the oneshot sender when Claude asks a question) and the
/// `agent_answer_question` command (which pops the sender to deliver answers).
/// The slot persists across turns so it doesn't need re-creating each time;
/// its contents are always `None` outside a pending question.
pub(crate) fn question_slot(state: &AppState, path: &Path) -> Result<QuestionSlot, AppError> {
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
pub(crate) fn permission_slot(state: &AppState, path: &Path) -> Result<PermissionSlot, AppError> {
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
pub(crate) fn interrupt_slot(state: &AppState, path: &Path) -> Result<InterruptSlot, AppError> {
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
pub(crate) fn derive_run_states(state: &AppState, projects: &mut [ProjectEntry]) {
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

/// Is a streaming agent turn currently in flight for `path`?
///
/// True when a `claude_sessions` entry exists AND its inner `tokio::Mutex` is
/// held (a turn is streaming). Checked via non-blocking `try_lock`. Used both by
/// `derive_run_states` (for `list_projects`/`rescan_project`) and by the
/// `agent_is_busy` command (for frontend reconciliation after navigation).
///
/// Outer-map guards are held only over the synchronous Arc lookup, never across
/// an `.await`.
pub(crate) fn project_busy(state: &AppState, path: &Path) -> bool {
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
