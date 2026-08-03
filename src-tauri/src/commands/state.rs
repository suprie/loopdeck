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

use crate::claude_session::{InterruptSlot, PermissionSlot, PlanSlot, QuestionSlot};
use crate::config::{AgentConfig, AgentHarness, GlobalConfig, ProjectEntry, RunState};
use crate::conversation;
use crate::error::AppError;
use crate::harness::HarnessSession;
use crate::paths;
use crate::permission::PermissionPolicy;
use crate::run_executor::RunHandle;
use crate::secrets;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    /// Per-project pending `ExitPlanMode` slots. When the agent (running under
    /// `plan` permission mode) finishes planning and asks to leave plan mode,
    /// the read loop parks on the slot's oneshot receiver; the
    /// `agent_answer_plan` command pops the sender here to deliver the user's
    /// Approve/Reject verdict and wake the parked turn. Mirrors
    /// `pending_permissions`.
    pub pending_plans: Mutex<HashMap<PathBuf, PlanSlot>>,
    /// Per-project interrupt slots for graceful Stop. The streaming read loop
    /// installs a fresh oneshot sender per turn and `select!`s on the
    /// receiver; `agent_interrupt` pops + fires the sender, the loop wakes and
    /// writes the `interrupt` control_request, ending the turn while keeping
    /// the live process (and its context) alive.
    pub interrupt_slots: Mutex<HashMap<PathBuf, InterruptSlot>>,
    /// Per-project handle for an in-progress queued run (`prd-run-queue`
    /// Phase 2). Presence of a key is the "a run is active" signal `queue_run`
    /// checks before starting another; the executor task removes its own
    /// entry when the run ends (completed, failed, or cancelled). Separate
    /// from `run-plan.yaml`'s on-disk phase statuses, which is the source of
    /// truth for *what happened*; this map only tracks *whether a live task
    /// is driving it right now*, so a restart (no entry, ever) is
    /// distinguishable from a still-running process.
    pub run_handles: Mutex<HashMap<PathBuf, RunHandle>>,
    /// Roots currently owned by a live multi-agent worker. This is an
    /// in-memory admission lock, deliberately separate from the durable
    /// manifest: a fresh app reconciles stale manifest state before accepting
    /// another run.
    pub multi_agent_active_runs: Mutex<HashSet<PathBuf>>,
    /// Serializes durable manifest transitions and linked-worktree creation
    /// for each project, preventing an interrupt/retry from racing a worker's
    /// queued → running transition.
    pub multi_agent_manifest_locks: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
}

pub(crate) fn acquire_multi_agent_run(state: &AppState, root: &Path) -> Result<(), AppError> {
    let mut active = state
        .multi_agent_active_runs
        .lock()
        .map_err(|_| AppError::LockError)?;
    if !active.insert(root.to_path_buf()) {
        return Err(AppError::RunPlan(
            "a multi-agent loop is already active for this project".into(),
        ));
    }
    Ok(())
}

pub(crate) fn release_multi_agent_run(state: &AppState, root: &Path) -> Result<(), AppError> {
    state
        .multi_agent_active_runs
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(root);
    Ok(())
}

pub(crate) fn multi_agent_manifest_lock(
    state: &AppState,
    root: &Path,
) -> Result<Arc<tokio::sync::Mutex<()>>, AppError> {
    let mut locks = state
        .multi_agent_manifest_locks
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(locks
        .entry(root.to_path_buf())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone())
}

/// Pop and fire the interrupt sender for `path`, if a turn is in flight.
/// Shared by `agent_interrupt` (user-initiated Stop) and `cancel_run`
/// (`prd-run-queue` Phase 2) — same mechanism, two initiators. Returns
/// whether a live turn was actually interrupted (vs. a no-op when the slot
/// was empty).
pub(crate) fn fire_interrupt(state: &AppState, path: &Path) -> Result<bool, AppError> {
    let guard = state
        .interrupt_slots
        .lock()
        .map_err(|_| AppError::LockError)?;
    Ok(guard
        .get(path)
        .and_then(|slot| slot.lock().ok().and_then(|mut g| g.take()))
        .map(|sender| sender.send(()).is_ok())
        .unwrap_or(false))
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
/// discovery menu. Read from the active harness's native project skill root:
/// `.claude/skills/<dir>/SKILL.md` for Claude or
/// `.agents/skills/<dir>/SKILL.md` for Codex.
///
/// `name` is the SKILL.md frontmatter `name` (e.g. `loopdeck:rust-expert`),
/// which is what the active harness invokes the skill by — so the frontend
/// inserts it verbatim as `/<name>`. It is distinct from `directory`, the
/// on-disk folder name (`loopdeck-rust-expert`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntry {
    /// Frontmatter `name` — the invocation token the active harness recognizes.
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

/// Read a named roster entry and inject its UUID-scoped auth token from the
/// local secrets file. This is the resolver used by multi-agent runs so one
/// agent can never receive another agent's credential.
pub(crate) fn resolve_agent_config_by_id(
    state: &AppState,
    id: &str,
) -> Result<AgentConfig, AppError> {
    let mut agent_config = state
        .config
        .lock()
        .map_err(|_| AppError::LockError)?
        .find_agent_config(id)
        .ok_or_else(|| AppError::Config(format!("agent config '{id}' was not found")))?
        .config
        .clone();
    agent_config.auth_token = secrets::load_agent_auth_token(id)?;
    Ok(agent_config)
}

/// Read the default agent config from the registry and inject the auth token
/// from the local secrets file.
///
/// The token is never stored in `config.yaml` (it lives in a separate
/// owner-only file — see `secrets`), so it must be resolved here, at spawn
/// time. The returned value is a local owned `AgentConfig` passed by reference
/// to `ClaudeSession::spawn`, which sets it as a child env var
/// (`ANTHROPIC_AUTH_TOKEN`) and then drops it — the plaintext token is never
/// held on the long-lived `Mutex<GlobalConfig>`.
///
/// No `agent` block yet (fresh install, Settings never saved) falls back to
/// `AgentConfig::default()` rather than erroring, so a user who sets nothing
/// still gets the plain `claude` CLI behaviour (its own login session, no
/// forced token/base_url). A missing secrets-file token likewise resolves to
/// `None`, preserving the prior behaviour where a user may rely on
/// `ANTHROPIC_AUTH_TOKEN` inherited from their shell.
pub(crate) fn resolve_agent_config(state: &AppState) -> Result<AgentConfig, AppError> {
    let default_id = state
        .config
        .lock()
        .map_err(|_| AppError::LockError)?
        .default_named_agent_config()
        .map(|agent| agent.id.clone());
    if let Some(id) = default_id {
        return resolve_agent_config_by_id(state, &id);
    }

    // Compatibility for tests and unusual startup paths before the one-time
    // registry migration has persisted an old singleton configuration.
    let mut legacy = state
        .config
        .lock()
        .map_err(|_| AppError::LockError)?
        .agent
        .clone()
        .unwrap_or_default();
    legacy.auth_token = secrets::load_auth_token()?;
    Ok(legacy)
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
        let current_session = arc.try_lock().ok().map(|mut session| {
            let harness = session.harness();
            let usable = session.is_usable();
            (harness, usable)
        });
        let should_replace = should_replace_cached_session(current_session, desired_harness);
        if should_replace {
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

/// Replace an idle cached session when its provider changed or it is unusable.
///
/// `None` means the inner mutex is busy, so the in-flight provider must be
/// preserved until the next send.
fn should_replace_cached_session(
    current_session: Option<(AgentHarness, bool)>,
    desired_harness: AgentHarness,
) -> bool {
    matches!(
        current_session,
        Some((current, usable)) if !usable || current != desired_harness
    )
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

/// Get (or create) the per-project `ExitPlanMode` approval slot.
///
/// The plan-approval counterpart of `permission_slot`: one `PlanSlot` per
/// project path, shared between the read loop (stores the oneshot sender when
/// the agent asks to leave plan mode) and the `agent_answer_plan` command
/// (pops the sender to deliver the verdict). Persistent across turns; always
/// `None` outside a pending plan approval.
pub(crate) fn plan_slot(state: &AppState, path: &Path) -> Result<PlanSlot, AppError> {
    let mut guard = state
        .pending_plans
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
/// - `Waiting` — there is a pending manual-approval (`pending_permissions`), a
///   pending `AskUserQuestion` (`pending_answers`), or a pending plan approval
///   (`pending_plans`) for the project path. The in-flight turn is parked
///   awaiting the user.
/// - `Working` — a `claude_sessions` entry exists for the path AND its inner
///   `tokio::Mutex` is currently held (a turn is streaming). We check by
///   `try_lock`; success ⇒ idle, failure ⇒ busy.
/// - `Idle` — no live session, or session exists but no turn is in flight.
///
/// Note: `Done` is intentionally never set here. We don't track "last turn
/// finished at" timestamps globally; the frontend derives its own transient
/// "done" affordance from streaming-result events if desired.
pub(crate) fn derive_run_states(state: &AppState, projects: &mut [ProjectEntry]) {
    // Slots are created once per project and reused across turns (see
    // `question_slot`/`permission_slot`/`plan_slot`), so their *keys* persist
    // forever once a project has ever had a prompt. A path is actually
    // pending only when its slot's inner `Option` is `Some` right now.
    fn currently_pending<T>(
        map: &std::collections::HashMap<PathBuf, Arc<Mutex<Option<T>>>>,
    ) -> Vec<PathBuf> {
        map.iter()
            .filter(|(_, slot)| matches!(slot.lock(), Ok(guard) if guard.is_some()))
            .map(|(path, _)| path.clone())
            .collect()
    }

    let pending_paths: std::collections::HashSet<PathBuf> = {
        let perms = state
            .pending_permissions
            .lock()
            .map_err(|_| AppError::LockError);
        let answers = state
            .pending_answers
            .lock()
            .map_err(|_| AppError::LockError);
        let plans = state.pending_plans.lock().map_err(|_| AppError::LockError);
        let (Ok(perms), Ok(answers), Ok(plans)) = (perms, answers, plans) else {
            // If we can't observe pending state, leave everything as-is.
            return;
        };
        currently_pending(&perms)
            .into_iter()
            .chain(currently_pending(&answers))
            .chain(currently_pending(&plans))
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_cached_session_is_reused_when_harness_matches() {
        assert!(!should_replace_cached_session(
            Some((AgentHarness::Codex, true)),
            AgentHarness::Codex,
        ));
    }

    #[test]
    fn idle_cached_session_is_replaced_when_harness_changes() {
        assert!(should_replace_cached_session(
            Some((AgentHarness::Claude, true)),
            AgentHarness::Codex,
        ));
    }

    #[test]
    fn unusable_cached_session_is_replaced_even_when_harness_matches() {
        assert!(should_replace_cached_session(
            Some((AgentHarness::Codex, false)),
            AgentHarness::Codex,
        ));
    }

    #[test]
    fn busy_cached_session_is_preserved_until_next_send() {
        assert!(!should_replace_cached_session(None, AgentHarness::Codex,));
    }

    fn empty_state() -> AppState {
        AppState {
            config: Mutex::new(GlobalConfig::default()),
            claude_sessions: Mutex::new(HashMap::new()),
            pending_answers: Mutex::new(HashMap::new()),
            pending_permissions: Mutex::new(HashMap::new()),
            pending_plans: Mutex::new(HashMap::new()),
            interrupt_slots: Mutex::new(HashMap::new()),
            run_handles: Mutex::new(HashMap::new()),
            multi_agent_active_runs: Mutex::new(HashSet::new()),
            multi_agent_manifest_locks: Mutex::new(HashMap::new()),
        }
    }

    #[test]
    fn multi_agent_admission_is_project_scoped_and_releasable() {
        let state = empty_state();
        let root = PathBuf::from("/tmp/loopdeck-multi-agent-lock");
        acquire_multi_agent_run(&state, &root).unwrap();
        assert!(acquire_multi_agent_run(&state, &root).is_err());
        release_multi_agent_run(&state, &root).unwrap();
        acquire_multi_agent_run(&state, &root).unwrap();
    }

    /// Regression: `question_slot`/`permission_slot`/`plan_slot` create their
    /// map entry once and reuse it across turns, so the slot's *key* survives
    /// forever after a project's first-ever prompt. A resolved (now-`None`)
    /// slot must NOT pin the project to `Waiting` on later calls.
    #[test]
    fn resolved_slot_does_not_pin_run_state_to_waiting() {
        let state = empty_state();
        let path = PathBuf::from("/tmp/resolved-slot-project");
        state
            .pending_permissions
            .lock()
            .unwrap()
            .insert(path.clone(), Arc::new(Mutex::new(None)));

        let mut projects = vec![ProjectEntry {
            path: path.clone(),
            ..Default::default()
        }];
        derive_run_states(&state, &mut projects);

        assert_eq!(projects[0].run_state, RunState::Idle);
    }

    #[test]
    fn still_pending_slot_reports_waiting() {
        let state = empty_state();
        let path = PathBuf::from("/tmp/pending-slot-project");
        state.pending_answers.lock().unwrap().insert(
            path.clone(),
            Arc::new(Mutex::new(Some(crate::claude_session::PendingQuestion {
                request_id: "req-1".into(),
                questions: Vec::new(),
                sender: None,
            }))),
        );

        let mut projects = vec![ProjectEntry {
            path: path.clone(),
            ..Default::default()
        }];
        derive_run_states(&state, &mut projects);

        assert_eq!(projects[0].run_state, RunState::Waiting);
    }
}
