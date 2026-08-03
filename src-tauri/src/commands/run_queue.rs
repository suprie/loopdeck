//! Run-queue IPC commands — `prd-run-queue.md` Phase 2.
//!
//! `queue_run` starts a detached sequential executor for a project's
//! `.loopdeck/run-plan.yaml`; `get_run_status` polls it; `cancel_run` stops
//! it. The executor loop itself (`execute_run`) lives here rather than in
//! `run_executor.rs` because it drives turns through `AppState` — the same
//! `claude_session` spawn/retry/transcript pipeline `commands::agent` uses
//! for a human-initiated "Start Loop" — and this module's sibling
//! `commands::agent` already owns that orchestration for the same reason
//! (see `run_executor.rs`'s module docs).

use super::agent::{
    mark_turn_terminal, start_fresh_and_record_streaming, start_fresh_and_record_streaming_in_root,
    StreamingRunOptions,
};
use super::state::{fire_interrupt, resolve_root, AppState};
use crate::agents::{ClaudeEvent, TokenBudget};
use crate::epic;
use crate::error::AppError;
use crate::execution::{self, ExecutionState, LoopOrigin};
use crate::git;
use crate::limits;
use crate::run_executor::{
    self, build_interview_prompt, build_phase_prompt, extract_draft_pr_url,
    extract_interview_answers, extract_verdict, ResolvedBudgets, RunHandle, RunVerdict,
};
use crate::runplan::{self, InterviewStatus, RunBudgets, RunPhaseStatus, RunPlan, StallPolicy};
use chrono::Utc;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Child;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::{info, warn};
use uuid::Uuid;

const RUN_QUEUE_EVENT: &str = "run-queue-event";

#[derive(Clone, Serialize)]
struct RunQueueEvent {
    project_path: String,
    execution_id: String,
    event: serde_json::Value,
}

#[derive(Serialize)]
pub struct RunQueueStatus {
    plan: Option<RunPlan>,
    active: bool,
}

fn emit_run_queue_terminal(app: &AppHandle, root: &Path, result: String, is_error: bool) {
    let event = ClaudeEvent::Result {
        text: String::new(),
        thinking: None,
        result,
        usage: None,
        is_error,
        duration_ms: 0,
        session_id: String::new(),
    };
    let Ok(event) = serde_json::to_value(event) else {
        warn!("failed to encode unattended run terminal activity");
        return;
    };
    if let Err(error) = app.emit(
        RUN_QUEUE_EVENT,
        RunQueueEvent {
            project_path: root.to_string_lossy().into_owned(),
            execution_id: "run-queue".into(),
            event,
        },
    ) {
        warn!("failed to emit unattended run terminal activity: {error}");
    }
}

#[derive(Debug, PartialEq, Eq)]
enum BootstrapStop {
    Completed,
    Cancelled,
    TimedOut,
}

/// Outcome of racing a phase turn against its wall-clock watchdog.
#[derive(Debug)]
enum WatchdogOutcome<T> {
    /// The turn finished within its budget — carries its own `Result`
    /// (a turn-level error is still possible; that's unrelated to the clock).
    Completed(Result<T, AppError>),
    /// The turn was still running when `watchdog_timeout` elapsed — the
    /// budget's top risk (a stuck phase burning the night unnoticed), caught.
    TimedOut,
}

/// Race `turn` against `watchdog_timeout`. Pure `tokio::time::timeout`
/// wrapper, factored out of `execute_run` so the "does a runaway phase
/// actually get caught by its clock" mechanism is unit-testable with a
/// deliberately stuck fixture future, the same way `wait_for_bootstrap_child`
/// lets the worktree-bootstrap watchdog be tested with a stuck `sleep 30`
/// child process. Side effects on a timeout (evicting the cached session so
/// its Drop reaps the child, marking the turn terminal) stay in the caller —
/// this function only makes the timeout-vs-real-work decision.
async fn race_with_watchdog<F, T>(watchdog_timeout: Duration, turn: F) -> WatchdogOutcome<T>
where
    F: std::future::Future<Output = Result<T, AppError>>,
{
    match tokio::time::timeout(watchdog_timeout, turn).await {
        Ok(result) => WatchdogOutcome::Completed(result),
        Err(_) => WatchdogOutcome::TimedOut,
    }
}

fn run_worktree_path(root: &Path, run_id: &str) -> PathBuf {
    root.parent()
        .unwrap_or(root)
        .join(".loopdeck-runs")
        .join(run_id)
}

fn run_branch_name(run_id: &str) -> String {
    format!(
        "run/{}",
        run_id.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "-")
    )
}

/// Create the one isolated worktree used by every phase in a run. The path and
/// branch are persisted before a turn begins, so a failed/terminated run stays
/// inspectable and a restart does not create a second branch.
fn ensure_worktree(root: &Path, plan: &mut RunPlan) -> Result<PathBuf, AppError> {
    if let Some(path) = &plan.environment.worktree_path {
        if git::worktree_list(root)
            .map_err(AppError::RunPlan)?
            .iter()
            .any(|known| known == path)
        {
            return Ok(path.clone());
        }
        return Err(AppError::RunPlan(format!(
            "run worktree {} is missing; preserving the run for inspection",
            path.display()
        )));
    }
    let path = run_worktree_path(root, &plan.id);
    let branch = run_branch_name(&plan.id);
    let worktree = git::worktree_add(root, &path, &branch).map_err(AppError::RunPlan)?;
    plan.environment.worktree_path = Some(worktree.path.clone());
    plan.environment.worktree_branch = Some(worktree.branch);
    Ok(worktree.path)
}

/// Fresh worktrees intentionally contain no ignored build artifacts. Bootstrap
/// Node dependencies before the first phase so verification fails early and in
/// the isolated tree, never against the user's main checkout.
async fn bootstrap_worktree(
    path: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<BootstrapStop, AppError> {
    if !path.join("package.json").is_file() || path.join("node_modules").is_dir() {
        return Ok(BootstrapStop::Completed);
    }
    let mut command = tokio::process::Command::new("npm");
    command.arg("ci").current_dir(path).kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| AppError::RunPlan(format!("failed to bootstrap npm dependencies: {e}")))?;

    let outcome = wait_for_bootstrap_child(&mut child, timeout, cancel).await?;
    if outcome != BootstrapStop::Completed {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    Ok(outcome)
}

async fn wait_for_bootstrap_child(
    child: &mut tokio::process::Child,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<BootstrapStop, AppError> {
    let cancellation = async {
        loop {
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::pin!(cancellation);

    let outcome = tokio::select! {
        status = child.wait() => {
            let status = status.map_err(|e| {
                AppError::RunPlan(format!("failed while waiting for npm ci: {e}"))
            })?;
            if !status.success() {
                return Err(AppError::RunPlan("npm ci failed in isolated worktree".into()));
            }
            BootstrapStop::Completed
        }
        _ = &mut cancellation => BootstrapStop::Cancelled,
        _ = tokio::time::sleep(timeout) => BootstrapStop::TimedOut,
    };

    Ok(outcome)
}

fn terminate_session(state: &AppState, path: &Path) -> Result<(), AppError> {
    let session = state
        .claude_sessions
        .lock()
        .map_err(|_| AppError::LockError)?
        .remove(path);
    drop(session);
    Ok(())
}

/// Holds macOS awake while a run is active. Other platforms retain the same
/// safe execution semantics and the UI documents the plugged-in/lid-open
/// fallback from the PRD.
struct KeepAwake(Option<Child>);

impl KeepAwake {
    fn acquire() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self(
                Command::new("/usr/bin/caffeinate")
                    .args(["-dimsu", "-w"])
                    .arg(std::process::id().to_string())
                    .spawn()
                    .ok(),
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self(None)
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn resolved_phase_token_cap(plan: &RunPlan) -> u64 {
    plan.budgets
        .per_phase_token_cap
        .unwrap_or(limits::DEFAULT_RUN_PHASE_TOKEN_CAP)
}

fn resolved_phase_timeout(plan: &RunPlan) -> Duration {
    Duration::from_secs(
        plan.budgets
            .per_phase_wall_clock_secs
            .unwrap_or(limits::DEFAULT_RUN_PHASE_WALL_CLOCK_SECS),
    )
}

fn resolved_run_timeout(plan: &RunPlan) -> Duration {
    Duration::from_secs(
        plan.budgets
            .total_run_wall_clock_secs
            .unwrap_or(limits::DEFAULT_RUN_TOTAL_WALL_CLOCK_SECS),
    )
}

fn validate_budgets(budgets: &RunBudgets) -> Result<(), AppError> {
    for (name, value) in [
        ("per-phase token cap", budgets.per_phase_token_cap),
        (
            "per-phase wall-clock cap",
            budgets.per_phase_wall_clock_secs,
        ),
        (
            "total-run wall-clock cap",
            budgets.total_run_wall_clock_secs,
        ),
    ] {
        if value == Some(0) {
            return Err(AppError::RunPlan(format!(
                "{name} must be greater than zero"
            )));
        }
    }
    Ok(())
}

/// Build and persist a new `.loopdeck/run-plan.yaml` from a picker selection
/// (Phase 5): the phases the user checked, in the order they were selected,
/// under one queue-time `stall_policy` and draft-PR consent.
///
/// Every ID is validated against `docs/epics/` (`epic::find_loop_by_id`) so a
/// stale or mistyped ID is rejected before it's written to disk, not
/// discovered later at run time. `depends_on` defaults to the authored
/// selection order — each phase depends on its immediate predecessor — per
/// the PRD's "linear chain, no editor" v1 design; there is no edge editor in
/// this phase. Every phase starts `Queued`/`Pending` (interview unanswered),
/// so `queue_run` still gates on the pre-flight interview as usual.
///
/// Replaces any existing plan outright — a fresh picker selection is always
/// meant to start a new plan, not merge into a stale one — but refuses while
/// a run is actively executing (same guard `queue_run` uses), so a picker
/// selection can't clobber the plan an in-flight executor is reading.
#[tauri::command]
pub async fn create_run_plan(
    path: String,
    execution_ids: Vec<String>,
    stall_policy: StallPolicy,
    draft_pr_authorized: bool,
    budgets: RunBudgets,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;
    validate_budgets(&budgets)?;

    if execution_ids.is_empty() {
        return Err(AppError::RunPlan(
            "select at least one phase to queue".into(),
        ));
    }

    {
        let handles = state.run_handles.lock().map_err(|_| AppError::LockError)?;
        if handles.contains_key(&root) {
            return Err(AppError::RunPlan(
                "a run is already in progress for this project — cancel it before queuing a new plan".into(),
            ));
        }
    }

    for id in &execution_ids {
        epic::find_loop_by_id(&root, id).ok_or_else(|| {
            AppError::RunPlan(format!(
                "no PRD checklist loop with id \"{id}\" found under docs/epics/"
            ))
        })?;
    }

    let mut plan = run_executor::build_run_plan(
        format!("run-{}", Uuid::new_v4()),
        root.clone(),
        Utc::now(),
        &execution_ids,
        stall_policy,
        draft_pr_authorized,
    );
    plan.budgets = budgets;

    runplan::save(&root, &plan)?;
    info!(
        "run plan queued for {} with {} phase(s)",
        root.display(),
        plan.phases.len()
    );
    Ok(plan)
}

/// Start executing a project's queued run plan in the background.
///
/// Returns immediately once the executor task is spawned — a run is
/// typically hours long, so the caller polls `get_run_status` rather than
/// awaiting completion. Refuses to start when: no plan is queued, a run is
/// already active for this project, no phase is `Queued`, or a loop is
/// already `current` in `execution.yaml` (queuing would race a manual loop).
#[tauri::command]
pub async fn queue_run(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let root = resolve_root(&state, &path)?;

    {
        let handles = state.run_handles.lock().map_err(|_| AppError::LockError)?;
        if handles.contains_key(&root) {
            return Err(AppError::RunPlan(
                "a run is already in progress for this project".into(),
            ));
        }
    }

    // A `Running` phase left over from a prior crash isn't actually running
    // on this fresh process — reconcile before deciding whether a run is
    // already active, so a stale status can't block queuing forever.
    run_executor::reconcile_running_phases(&root)?;

    let plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan is queued for this project".into()))?;
    reconcile_inactive_execution(&root, &plan)?;

    if !plan
        .phases
        .iter()
        .any(|p| p.status == RunPhaseStatus::Queued)
    {
        return Err(AppError::RunPlan(
            "the run plan has no queued phases".into(),
        ));
    }

    if let Some(phase) = plan.phases.iter().find(|p| {
        p.status == RunPhaseStatus::Queued && p.interview_status == InterviewStatus::Pending
    }) {
        return Err(AppError::RunPlan(format!(
            "phase \"{}\" has not been interviewed — answer or skip its pre-flight interview before queuing the run",
            phase.execution_id
        )));
    }

    if execution::load(&root)?.state.current.is_some() {
        return Err(AppError::RunPlan(
            "a loop is already active in execution.yaml — complete or abandon it before queuing a run".into(),
        ));
    }

    let handle = RunHandle::new();
    let cancel = Arc::clone(&handle.cancel);
    {
        let mut handles = state.run_handles.lock().map_err(|_| AppError::LockError)?;
        handles.insert(root.clone(), handle);
    }

    let app_handle = app.clone();
    let run_root = root.clone();
    tokio::spawn(async move {
        let state = app_handle.state::<AppState>();
        let outcome = execute_run(&app_handle, &state, &run_root, &cancel).await;
        let (terminal_message, is_error) = match outcome {
            Ok(()) => ("Run queue finished".into(), false),
            Err(error) => {
                warn!(
                    "run queue executor for {} ended with error: {error}",
                    run_root.display()
                );
                (error.to_string(), true)
            }
        };
        if let Ok(mut handles) = state.run_handles.lock() {
            handles.remove(&run_root);
        };
        emit_run_queue_terminal(&app_handle, &run_root, terminal_message, is_error);
    });

    info!("queued run started for {}", root.display());
    Ok(())
}

/// Cancel the in-progress run for a project. Fires the run's cancel flag,
/// checked between phases — the earliest point the executor's loop can react.
///
/// Also fires the project's `interrupt_slots` sender via `fire_interrupt`, the
/// same one `agent_interrupt` uses for a user-initiated Stop. Since Phase 4
/// moved the executor onto the streaming pipeline
/// (`start_fresh_and_record_streaming`), this now genuinely interrupts the
/// current phase's turn **while it is actively reading** — the streaming read
/// loop `select!`s the next stdout line against the interrupt receiver (see
/// `claude_session.rs::send_message_streaming`). It still has **no effect on
/// an already-parked card** (an unanswered `AskUserQuestion`/permission/plan
/// approval): `answer_control_request`'s park site isn't selected against
/// anything but its own deadline once entered, so a cancel during a park
/// waits out `TURN_DEADLINE` like any other unattended stall (see
/// `run_executor::phases_blocked_by_park`'s caller for the same limit). No-op
/// error if no run is active.
#[tauri::command]
pub async fn cancel_run(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let root = resolve_root(&state, &path)?;

    let found = {
        let handles = state.run_handles.lock().map_err(|_| AppError::LockError)?;
        handles
            .get(&root)
            .map(|h| h.cancel.store(true, Ordering::SeqCst))
            .is_some()
    };
    if !found {
        return Err(AppError::RunPlan(
            "no run is in progress for this project".into(),
        ));
    }

    // Phase sessions run in the isolated worktree, while legacy/manual
    // sessions are keyed by the registered root. Signal both safely so Stop
    // remains immediate during the migration to worktree-backed runs.
    let _ = fire_interrupt(&state, &root)?;
    if let Some(worktree) = runplan::load(&root)?.and_then(|plan| plan.environment.worktree_path) {
        let _ = fire_interrupt(&state, &worktree)?;
    }
    info!("cancel_run fired for {}", root.display());
    Ok(())
}

/// Read the current run plan (and thus its live per-phase status) for a
/// project. `None` when no plan has ever been queued. Reconciles a stale
/// `Running` phase (process restarted since it was marked) when this project
/// has no live executor handle in memory.
#[tauri::command]
pub async fn get_run_status(
    path: String,
    state: State<'_, AppState>,
) -> Result<RunQueueStatus, AppError> {
    let root = resolve_root(&state, &path)?;
    let active = state
        .run_handles
        .lock()
        .map_err(|_| AppError::LockError)?
        .contains_key(&root);
    if !active {
        run_executor::reconcile_running_phases(&root)?;
    }
    let plan = runplan::load(&root)?;
    if !active {
        if let Some(plan) = &plan {
            reconcile_inactive_execution(&root, plan)?;
        }
    }
    Ok(RunQueueStatus { plan, active })
}

fn reconcile_inactive_execution(root: &Path, plan: &RunPlan) -> Result<(), AppError> {
    let loaded = execution::load(root)?;
    if let Some(next) = reconcile_inactive_execution_state(&loaded.state, plan, Utc::now())? {
        execution::save(root, &next, loaded.state.revision)?;
    }
    Ok(())
}

fn reconcile_inactive_execution_state(
    state: &ExecutionState,
    plan: &RunPlan,
    now: chrono::DateTime<Utc>,
) -> Result<Option<ExecutionState>, AppError> {
    let Some(current) = state.current.as_ref() else {
        return Ok(None);
    };
    let Some(phase) = plan
        .phases
        .iter()
        .find(|phase| phase.execution_id == current.id)
    else {
        return Ok(None);
    };

    let next = match phase.status {
        RunPhaseStatus::Completed => state.complete_current(now, None, false)?,
        RunPhaseStatus::Parked
        | RunPhaseStatus::Failed
        | RunPhaseStatus::Interrupted
        | RunPhaseStatus::Killed => state.abandon_current(
            format!(
                "reconciled inactive run phase \"{}\" in {:?} state",
                phase.execution_id, phase.status
            ),
            now,
            false,
        )?,
        RunPhaseStatus::Queued | RunPhaseStatus::Running => return Ok(None),
    };
    Ok(Some(next))
}

/// Requeue one retryable terminal phase and any phases parked only because
/// they depend on it. The next `queue_run` starts a fresh unattended turn.
#[tauri::command]
pub async fn requeue_run_phase(
    path: String,
    execution_id: String,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;
    if state
        .run_handles
        .lock()
        .map_err(|_| AppError::LockError)?
        .contains_key(&root)
    {
        return Err(AppError::RunPlan(
            "cannot retry a phase while the run is still active".into(),
        ));
    }

    let mut plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan exists for this project".into()))?;
    requeue_terminal_phase(&mut plan, &execution_id)?;
    runplan::save(&root, &plan)?;
    Ok(plan)
}

fn requeue_terminal_phase(plan: &mut RunPlan, execution_id: &str) -> Result<(), AppError> {
    let Some(phase) = plan
        .phases
        .iter_mut()
        .find(|phase| phase.execution_id == execution_id)
    else {
        return Err(AppError::RunPlan(format!(
            "phase \"{execution_id}\" is not part of this run"
        )));
    };
    if !matches!(
        phase.status,
        RunPhaseStatus::Parked
            | RunPhaseStatus::Failed
            | RunPhaseStatus::Interrupted
            | RunPhaseStatus::Killed
    ) {
        return Err(AppError::RunPlan(format!(
            "phase \"{execution_id}\" is not retryable"
        )));
    }
    phase.status = RunPhaseStatus::Queued;
    phase.park_payload = None;

    let mut requeued = HashSet::from([execution_id.to_string()]);
    loop {
        let mut changed = false;
        for phase in &mut plan.phases {
            let blocked_by_requeued_phase = phase.status == RunPhaseStatus::Parked
                && phase
                    .park_payload
                    .as_deref()
                    .is_some_and(|payload| payload.starts_with("blocked: depends on parked phase"))
                && phase.depends_on.iter().any(|id| requeued.contains(id));
            if blocked_by_requeued_phase {
                phase.status = RunPhaseStatus::Queued;
                phase.park_payload = None;
                requeued.insert(phase.execution_id.clone());
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

fn park_blocked_dependents(plan: &mut RunPlan, execution_id: &str) {
    for blocked_id in run_executor::phases_blocked_by_park(plan, execution_id, plan.stall_policy) {
        if let Some(phase) = plan
            .phases
            .iter_mut()
            .find(|phase| phase.execution_id == blocked_id)
        {
            phase.status = RunPhaseStatus::Parked;
            phase.park_payload = Some(format!(
                "blocked: depends on parked phase \"{execution_id}\""
            ));
        }
    }
}

/// Run one queued phase's pre-flight interview turn (Phase 3) — a bounded
/// session driven through the *streaming* pipeline
/// (`start_fresh_and_record_streaming`, no-op sink `Channel`, same trick
/// Phase 4's executor uses). This is load-bearing, not cosmetic: per
/// `claude_session.rs::answer_ask_user_question`'s own doc comment, an
/// `AskUserQuestion` on the *non*-streaming path (`channel: None`) has no UI
/// surface to answer from and is auto-denied immediately instead of parking —
/// exactly the tool call `build_interview_prompt` tells the agent to make. A
/// channel merely being present (its callback can be a no-op) is what lets
/// `answer_control_request` park on the shared `question_slot` instead of
/// taking that deny branch; the pending card then surfaces through the
/// existing tab-agnostic `StuckQuestionCallout` (`ProjectDetail.tsx`), which
/// reads the same cross-project `AskUserQuestion` store every other pending
/// question does — no bespoke card needed in the run-queue UI. Awaits the
/// whole turn, including any parked question, so it only returns once the
/// user has answered (or the agent decided nothing was ambiguous) — that's
/// what "while the user is present" means here: the caller is expected to be
/// an active UI session, not a background poll.
///
/// Pins the parsed answers into `plan.phases[execution_id].interview`, marks
/// its `interview_status` `Answered` (even when zero questions were asked —
/// that's still a resolved interview, distinct from `Pending`), and persists
/// the plan. Errors if no plan is queued or the phase isn't found in it;
/// does not require the phase to still be `Queued` (re-running an interview
/// on a `Parked`/`Failed` phase before a retry is allowed).
#[tauri::command]
pub async fn run_phase_interview(
    path: String,
    execution_id: String,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;

    let plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan is queued for this project".into()))?;
    if !plan.phases.iter().any(|p| p.execution_id == execution_id) {
        return Err(AppError::RunPlan(format!(
            "phase \"{execution_id}\" is not in the run plan"
        )));
    }

    let loc = epic::find_loop_by_id(&root, &execution_id).ok_or_else(|| {
        AppError::RunPlan(format!(
            "queued phase \"{execution_id}\" no longer exists in docs/epics/"
        ))
    })?;

    let prompt = build_interview_prompt(&execution_id, &loc);
    // No-op sink: nothing here needs to narrate turn events to a UI channel —
    // only the channel's *presence* matters, so a parked AskUserQuestion is
    // answerable instead of auto-denied (see this fn's doc comment).
    let channel: Channel<ClaudeEvent> = Channel::new(|_| Ok(()));
    let response =
        start_fresh_and_record_streaming(&state, &root, &prompt, Some(loc.title.clone()), &channel)
            .await?;
    let answers = extract_interview_answers(&response.result);

    // Reload rather than reuse the plan loaded before the (possibly long)
    // interview turn — `cancel_run`/the executor could have touched other
    // phases meanwhile; only this phase's fields are ours to set.
    let mut plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("run plan disappeared during interview".into()))?;
    let idx = plan
        .phases
        .iter()
        .position(|p| p.execution_id == execution_id)
        .ok_or_else(|| {
            AppError::RunPlan(format!(
                "phase \"{execution_id}\" was removed from the run plan during its interview"
            ))
        })?;
    plan.phases[idx].interview = answers;
    plan.phases[idx].interview_status = InterviewStatus::Answered;
    runplan::save(&root, &plan)?;

    info!(
        "interview answered for phase \"{execution_id}\" in {}",
        root.display()
    );
    Ok(plan)
}

/// Explicitly skip a queued phase's pre-flight interview — no session is
/// run, `interview` is left as-is (typically empty), `interview_status`
/// becomes `Skipped`. Lets the user unblock `queue_run` for a phase they've
/// judged unambiguous without opening a turn for it.
#[tauri::command]
pub async fn skip_phase_interview(
    path: String,
    execution_id: String,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;

    let mut plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan is queued for this project".into()))?;
    let idx = plan
        .phases
        .iter()
        .position(|p| p.execution_id == execution_id)
        .ok_or_else(|| {
            AppError::RunPlan(format!("phase \"{execution_id}\" is not in the run plan"))
        })?;

    plan.phases[idx].interview_status = InterviewStatus::Skipped;
    runplan::save(&root, &plan)?;

    info!(
        "interview skipped for phase \"{execution_id}\" in {}",
        root.display()
    );
    Ok(plan)
}

/// The sequential executor loop: one orchestrated turn per queued phase, in
/// the plan's authored (vec) order.
///
/// **Interactive-stall handling (Phase 4)** — each phase's turn runs through
/// the *streaming* pipeline (`start_fresh_and_record_streaming`, with a
/// no-op sink `Channel`) rather than Phase 2/3's non-streaming
/// `start_fresh_and_record`. This isn't cosmetic: `claude_session.rs`'s
/// `answer_control_request` only parks an `AskUserQuestion` / manual-approval
/// / plan-approval card when a channel is present — on the non-streaming path
/// (`channel: None`) it auto-denies the card immediately instead of parking.
/// Streaming is what makes a genuine mid-run stall observable at all. The
/// no-op channel means this run has no live UI narration of its own (Phase
/// 5's run-queue view), but the pending card still lands in the same shared
/// `AppState` slots `agent_pending_question`/`_permission`/`_plan` read — a
/// human watching the app live can answer it exactly like any other pending
/// card, and the turn then completes normally (never reaching the
/// `TurnParked` arm below at all).
///
/// A stalled card is only detectable once it exceeds `TURN_DEADLINE` (30
/// min): `claude_session.rs`'s park site is not selected against any
/// cancellation once entered (see its own doc comment — "during a parked
/// approval/question the loop is off the read, so an interrupt there won't
/// be observed this turn"), so there's no way to notice or bound a stall
/// earlier from outside `claude_session.rs` without a larger session-model
/// change (out of this PRD's scope — sequencing/state, not environment).
/// `park the phase instead of waiting` (PRD Phase 4) means "instead of the
/// *run* waiting forever / stopping outright," per the `TurnParked` handling
/// below — not that detection is instant.
///
/// On `AppError::TurnParked`, the phase becomes `Parked` (not `Failed`) with
/// the pending card's payload recorded for the morning report, and
/// [`run_executor::phases_blocked_by_park`] marks the phases the plan's
/// `StallPolicy` says can't proceed: under `ContinueIndependent` that's only
/// the parked phase's dependents (everything else stays `Queued` and the loop
/// tries it next); under `Halt` that's every remaining `Queued` phase, which
/// leaves nothing for the loop to pick up — no separate early-return branch
/// needed. Either way `execution.yaml`'s `current` loop is abandoned (not
/// left dangling) so the next phase (if any) can be promoted — by the time
/// `continue_independent` reaches a next phase, that phase's `spawn_fresh`
/// would replace the parked session anyway (Phase 2's existing "fresh
/// process per phase" design), so there is no still-resumable session to
/// preserve by leaving `current` untouched.
///
/// A non-green verify verdict or any other turn-level error is unchanged
/// from Phase 2: the phase is `Failed` and the run stops.
async fn execute_run(
    app: &AppHandle,
    state: &AppState,
    root: &Path,
    cancel: &Arc<AtomicBool>,
) -> Result<(), AppError> {
    let Some(mut plan) = runplan::load(root)? else {
        return Ok(());
    };
    let _keep_awake = KeepAwake::acquire();
    let run_started = Instant::now();
    let worktree = ensure_worktree(root, &mut plan)?;
    runplan::save(root, &plan)?;
    let bootstrap_timeout = resolved_run_timeout(&plan).saturating_sub(run_started.elapsed());
    match bootstrap_worktree(&worktree, bootstrap_timeout, cancel).await {
        Ok(BootstrapStop::Completed) => {}
        Ok(BootstrapStop::Cancelled) | Ok(BootstrapStop::TimedOut) => {
            let reason = if cancel.load(Ordering::SeqCst) {
                "run cancelled during worktree bootstrap"
            } else {
                "total run wall-clock budget exceeded during worktree bootstrap"
            };
            if let Some(phase) = plan
                .phases
                .iter_mut()
                .find(|phase| phase.status == RunPhaseStatus::Queued)
            {
                phase.status = RunPhaseStatus::Killed;
                phase.park_payload = Some(reason.into());
            }
            plan.wall_clock_secs = run_started.elapsed().as_secs();
            plan.environment.worktree_kept = true;
            runplan::save(root, &plan)?;
            return Ok(());
        }
        Err(error) => {
            if let Some(phase) = plan
                .phases
                .iter_mut()
                .find(|phase| phase.status == RunPhaseStatus::Queued)
            {
                phase.status = RunPhaseStatus::Failed;
                phase.park_payload = Some(error.to_string());
            }
            plan.environment.worktree_kept = true;
            runplan::save(root, &plan)?;
            return Err(error);
        }
    }

    for idx in 0..plan.phases.len() {
        if plan.phases[idx].status != RunPhaseStatus::Queued {
            continue;
        }
        plan.wall_clock_secs = run_started.elapsed().as_secs();
        if run_started.elapsed() >= resolved_run_timeout(&plan) {
            plan.phases[idx].status = RunPhaseStatus::Killed;
            plan.phases[idx].park_payload = Some("total run wall-clock budget exceeded".into());
            plan.environment.worktree_kept = true;
            runplan::save(root, &plan)?;
            return Ok(());
        }
        if cancel.load(Ordering::SeqCst) {
            plan.phases[idx].status = RunPhaseStatus::Killed;
            plan.phases[idx].park_payload = Some("run cancelled".into());
            plan.environment.worktree_kept = true;
            runplan::save(root, &plan)?;
            return Ok(());
        }

        let execution_id = plan.phases[idx].execution_id.clone();
        let interview = plan.phases[idx].interview.clone();

        let loc = match epic::find_loop_by_id(root, &execution_id) {
            Some(loc) => loc,
            None => {
                plan.phases[idx].status = RunPhaseStatus::Failed;
                plan.environment.worktree_kept = true;
                runplan::save(root, &plan)?;
                return Err(AppError::RunPlan(format!(
                    "queued phase \"{execution_id}\" no longer exists in docs/epics/"
                )));
            }
        };

        // Mark running in both the plan and execution.yaml before spawning
        // the turn, so a crash mid-turn leaves truthful on-disk state
        // (reconciled to Interrupted/back-to-current by the next read) rather
        // than a phase that silently never started.
        plan.phases[idx].status = RunPhaseStatus::Running;
        runplan::save(root, &plan)?;

        let loaded = execution::load(root)?;
        let promoted = loaded.state.promote_loop_into_current(
            &execution_id,
            &loc.title,
            LoopOrigin {
                epic: loc.epic.clone(),
                prd: loc.prd.clone(),
                phase: loc.phase.clone(),
            },
            chrono::Utc::now(),
        )?;
        execution::save(root, &promoted, loaded.state.revision)?;

        let phase_token_cap = resolved_phase_token_cap(&plan);
        let phase_timeout = resolved_phase_timeout(&plan);
        let run_timeout = resolved_run_timeout(&plan);
        let prompt = build_phase_prompt(
            &execution_id,
            &loc,
            &interview,
            plan.consent.draft_pr_authorized,
            ResolvedBudgets {
                phase_token_cap,
                phase_wall_clock_secs: phase_timeout.as_secs(),
                run_wall_clock_secs: run_timeout.as_secs(),
            },
        );
        let event_app = app.clone();
        let event_project_path = root.to_string_lossy().into_owned();
        let event_execution_id = execution_id.clone();
        let channel: Channel<ClaudeEvent> = Channel::new(move |event| {
            let event = match event.deserialize::<serde_json::Value>() {
                Ok(event) => event,
                Err(error) => {
                    warn!("failed to decode unattended run activity: {error}");
                    return Ok(());
                }
            };
            if let Err(error) = event_app.emit(
                RUN_QUEUE_EVENT,
                RunQueueEvent {
                    project_path: event_project_path.clone(),
                    execution_id: event_execution_id.clone(),
                    event,
                },
            ) {
                warn!("failed to emit unattended run activity: {error}");
            }
            Ok(())
        });
        let token_budget = TokenBudget::new(phase_token_cap);
        let phase_started = Instant::now();
        let run_remaining = run_timeout.saturating_sub(run_started.elapsed());
        let (watchdog_timeout, watchdog_reason) = if run_remaining <= phase_timeout {
            (run_remaining, "total run wall-clock budget exceeded")
        } else {
            (phase_timeout, "phase wall-clock budget exceeded")
        };
        let turn = start_fresh_and_record_streaming_in_root(
            state,
            &worktree,
            root,
            &prompt,
            Some(loc.title.clone()),
            &channel,
            StreamingRunOptions {
                token_budget: Some(&token_budget),
                force_autonomous: true,
            },
        );
        let outcome = match race_with_watchdog(watchdog_timeout, turn).await {
            WatchdogOutcome::Completed(result) => result,
            WatchdogOutcome::TimedOut => {
                // `timeout` (inside `race_with_watchdog`) drops the turn future
                // first, releasing its session lock. Removing the last cached
                // Arc then invokes the harness Drop path: graceful EOF, bounded
                // wait, and SIGKILL fallback.
                terminate_session(state, &worktree)?;
                let error = AppError::RunPlan(format!(
                    "{watchdog_reason} after {} seconds",
                    watchdog_timeout.as_secs()
                ));
                mark_turn_terminal(root, &error);
                Err(error)
            }
        };
        plan.phases[idx].wall_clock_secs = phase_started.elapsed().as_secs();
        plan.wall_clock_secs = run_started.elapsed().as_secs();
        plan.phases[idx].token_usage = token_budget.used();

        match outcome {
            Ok(response) => {
                let tokens = response
                    .usage
                    .as_ref()
                    .map(|usage| usage.input_tokens.saturating_add(usage.output_tokens))
                    .unwrap_or_default();
                let exceeded = token_budget.observe_total(tokens);
                plan.phases[idx].token_usage = token_budget.used();
                if exceeded {
                    plan.phases[idx].status = RunPhaseStatus::Killed;
                    plan.phases[idx].park_payload = Some(token_budget.exceeded_message());
                    plan.environment.worktree_kept = true;
                    runplan::save(root, &plan)?;
                    let loaded = execution::load(root)?;
                    let abandoned = loaded.state.abandon_current(
                        "phase token budget exceeded",
                        chrono::Utc::now(),
                        false,
                    )?;
                    execution::save(root, &abandoned, loaded.state.revision)?;
                    return Ok(());
                }
                let verdict = extract_verdict(&response.result);
                if verdict == Some(RunVerdict::Pass) && plan.consent.draft_pr_authorized {
                    let Some(url) = extract_draft_pr_url(&response.result) else {
                        let reason = "green verification passed, but no unattended draft PR URL was recorded";
                        plan.phases[idx].status = RunPhaseStatus::Parked;
                        plan.phases[idx].park_payload = Some(reason.into());
                        plan.environment.worktree_kept = true;
                        park_blocked_dependents(&mut plan, &execution_id);
                        runplan::save(root, &plan)?;
                        let loaded = execution::load(root)?;
                        let abandoned =
                            loaded
                                .state
                                .abandon_current(reason, chrono::Utc::now(), false)?;
                        execution::save(root, &abandoned, loaded.state.revision)?;
                        continue;
                    };
                    plan.phases[idx].status = RunPhaseStatus::Completed;
                    plan.phases[idx].park_payload = Some(format!("draft PR: {url}"));
                    runplan::save(root, &plan)?;

                    let loaded = execution::load(root)?;
                    let completed =
                        loaded
                            .state
                            .complete_current(chrono::Utc::now(), None, false)?;
                    execution::save(root, &completed, loaded.state.revision)?;
                } else {
                    let reason = match verdict {
                        Some(RunVerdict::Pass) => "green verification passed, but queue-time consent for an unattended draft PR is required".to_string(),
                        Some(RunVerdict::Warn) => "verify verdict: WARN".to_string(),
                        Some(RunVerdict::Block) => "verify verdict: BLOCK".to_string(),
                        _ => "no verify verdict found in the turn's final response".to_string(),
                    };
                    plan.phases[idx].status = RunPhaseStatus::Parked;
                    plan.phases[idx].park_payload = Some(reason.clone());
                    plan.environment.worktree_kept = true;
                    park_blocked_dependents(&mut plan, &execution_id);
                    runplan::save(root, &plan)?;

                    let loaded = execution::load(root)?;
                    let abandoned =
                        loaded
                            .state
                            .abandon_current(reason, chrono::Utc::now(), false)?;
                    execution::save(root, &abandoned, loaded.state.revision)?;
                    continue;
                }
            }
            Err(AppError::TurnParked(detail)) => {
                plan.phases[idx].status = RunPhaseStatus::Parked;
                plan.phases[idx].park_payload = Some(detail.clone());
                plan.environment.worktree_kept = true;
                park_blocked_dependents(&mut plan, &execution_id);
                runplan::save(root, &plan)?;

                let loaded = execution::load(root)?;
                let abandoned = loaded.state.abandon_current(
                    format!("phase parked: {detail}"),
                    chrono::Utc::now(),
                    false,
                )?;
                execution::save(root, &abandoned, loaded.state.revision)?;
                // No early return: under `ContinueIndependent` the loop
                // naturally advances to try the next still-`Queued` phase;
                // under `Halt`, `phases_blocked_by_park` already parked every
                // remaining phase, so the top-of-iteration status check skips
                // them and the run ends here regardless.
            }
            Err(e) => {
                let killed_by_budget = e.to_string().contains("wall-clock budget exceeded")
                    || e.to_string().contains("token budget exceeded");
                plan.phases[idx].status = if killed_by_budget {
                    RunPhaseStatus::Killed
                } else {
                    RunPhaseStatus::Failed
                };
                plan.phases[idx].park_payload = Some(e.to_string());
                plan.environment.worktree_kept = true;
                runplan::save(root, &plan)?;

                if e.to_string().contains("token budget exceeded") {
                    terminate_session(state, &worktree)?;
                }

                let loaded = execution::load(root)?;
                let abandoned =
                    loaded
                        .state
                        .abandon_current(e.to_string(), chrono::Utc::now(), false)?;
                execution::save(root, &abandoned, loaded.state.revision)?;
                return if killed_by_budget { Ok(()) } else { Err(e) };
            }
        }
    }

    plan.wall_clock_secs = run_started.elapsed().as_secs();
    finalize_worktree(root, &worktree, &mut plan);
    runplan::save(root, &plan)?;

    Ok(())
}

/// Cleanup policy for the run's isolated worktree, applied once every phase
/// has been attempted (`prd-unattended-ship.md` P0: "prune on success, keep +
/// flag on failure/kill"). Split out of `execute_run` so the decision itself
/// — prune vs. keep, and keep-on-prune-failure — is testable against a real
/// worktree without needing a live `claude_session` turn.
fn finalize_worktree(root: &Path, worktree: &Path, plan: &mut RunPlan) {
    if plan
        .phases
        .iter()
        .all(|phase| phase.status == RunPhaseStatus::Completed)
    {
        if let Err(error) = git::worktree_remove(root, worktree) {
            plan.environment.worktree_kept = true;
            if let Some(phase) = plan.phases.last_mut() {
                phase.park_payload = Some(format!(
                    "draft PR succeeded, but worktree cleanup failed: {error}"
                ));
            }
        } else {
            plan.environment.worktree_path = None;
            plan.environment.worktree_branch = None;
        }
    } else {
        plan.environment.worktree_kept = true;
    }
}

#[cfg(test)]
mod unattended_tests {
    use super::*;
    use chrono::Utc;

    fn plan() -> RunPlan {
        run_executor::build_run_plan(
            "run-budget-test".into(),
            PathBuf::from("/repo"),
            Utc::now(),
            &["phase-1".into()],
            StallPolicy::ContinueIndependent,
            false,
        )
    }

    fn parked_chain() -> RunPlan {
        run_executor::build_run_plan(
            "run-requeue-test".into(),
            PathBuf::from("/repo"),
            Utc::now(),
            &["phase-a".into(), "phase-b".into(), "phase-c".into()],
            StallPolicy::ContinueIndependent,
            false,
        )
    }

    fn active_execution(id: &str) -> ExecutionState {
        ExecutionState::default()
            .promote_loop_into_current(
                id,
                "Test phase",
                LoopOrigin {
                    epic: "test-epic".into(),
                    prd: "test-prd".into(),
                    phase: "Phase 1".into(),
                },
                Utc::now(),
            )
            .expect("test phase should promote")
    }

    #[test]
    fn inactive_parked_queue_phase_clears_matching_execution_current() {
        let mut plan = parked_chain();
        plan.phases[0].status = RunPhaseStatus::Parked;
        let state = active_execution("phase-a");

        let reconciled = reconcile_inactive_execution_state(&state, &plan, Utc::now())
            .expect("reconciliation should succeed")
            .expect("terminal queue phase should reconcile");

        assert!(reconciled.current.is_none());
        let history = reconciled.history.last().expect("abandon record");
        assert_eq!(history.id, "phase-a");
        assert_eq!(history.outcome, crate::execution::Outcome::Abandoned);
    }

    #[test]
    fn inactive_completed_queue_phase_completes_matching_execution_current() {
        let mut plan = parked_chain();
        plan.phases[0].status = RunPhaseStatus::Completed;
        let state = active_execution("phase-a");

        let reconciled = reconcile_inactive_execution_state(&state, &plan, Utc::now())
            .expect("reconciliation should succeed")
            .expect("completed queue phase should reconcile");

        assert!(reconciled.current.is_none());
        assert_eq!(
            reconciled
                .history
                .last()
                .expect("completion record")
                .outcome,
            crate::execution::Outcome::Completed
        );
    }

    #[test]
    fn inactive_queue_reconciliation_never_clears_an_unrelated_manual_loop() {
        let plan = parked_chain();
        let state = active_execution("manual-loop");

        let reconciled = reconcile_inactive_execution_state(&state, &plan, Utc::now())
            .expect("reconciliation should succeed");

        assert!(reconciled.is_none());
        assert_eq!(state.current.as_ref().unwrap().id, "manual-loop");
    }

    #[test]
    fn inactive_queue_reconciliation_keeps_a_queued_current_phase() {
        let plan = parked_chain();
        let state = active_execution("phase-a");

        let reconciled = reconcile_inactive_execution_state(&state, &plan, Utc::now())
            .expect("reconciliation should succeed");

        assert!(reconciled.is_none());
        assert_eq!(state.current.as_ref().unwrap().id, "phase-a");
    }

    #[test]
    fn retrying_a_parked_phase_requeues_dependents_blocked_only_by_it() {
        let mut plan = parked_chain();
        plan.phases[0].status = RunPhaseStatus::Parked;
        plan.phases[0].park_payload = Some("approval required".into());
        for phase in &mut plan.phases[1..] {
            phase.status = RunPhaseStatus::Parked;
            phase.park_payload = Some("blocked: depends on parked phase phase-a".into());
        }

        requeue_terminal_phase(&mut plan, "phase-a").expect("parked phase should be retryable");

        assert!(plan
            .phases
            .iter()
            .all(|phase| phase.status == RunPhaseStatus::Queued));
        assert!(plan.phases.iter().all(|phase| phase.park_payload.is_none()));
    }

    #[test]
    fn retrying_a_parked_phase_preserves_independent_parks() {
        let mut plan = parked_chain();
        plan.phases[2].depends_on.clear();
        plan.phases[0].status = RunPhaseStatus::Parked;
        plan.phases[0].park_payload = Some("approval required".into());
        plan.phases[1].status = RunPhaseStatus::Parked;
        plan.phases[1].park_payload = Some("blocked: depends on parked phase phase-a".into());
        plan.phases[2].status = RunPhaseStatus::Parked;
        plan.phases[2].park_payload = Some("independent failure".into());

        requeue_terminal_phase(&mut plan, "phase-a").expect("parked phase should be retryable");

        assert_eq!(plan.phases[0].status, RunPhaseStatus::Queued);
        assert_eq!(plan.phases[1].status, RunPhaseStatus::Queued);
        assert_eq!(plan.phases[2].status, RunPhaseStatus::Parked);
        assert_eq!(
            plan.phases[2].park_payload.as_deref(),
            Some("independent failure")
        );
    }

    #[test]
    fn retry_rejects_a_phase_that_is_not_retryable() {
        let mut plan = parked_chain();
        let error = requeue_terminal_phase(&mut plan, "phase-a")
            .expect_err("queued phase must not be silently restarted");
        assert!(error.to_string().contains("is not retryable"));
    }

    #[test]
    fn retry_accepts_a_failed_or_interrupted_phase() {
        for status in [RunPhaseStatus::Failed, RunPhaseStatus::Interrupted] {
            let mut plan = parked_chain();
            plan.phases[0].status = status;
            plan.phases[0].park_payload = Some("turn interrupted".into());

            requeue_terminal_phase(&mut plan, "phase-a")
                .expect("failed and interrupted phases should be retryable");

            assert_eq!(plan.phases[0].status, RunPhaseStatus::Queued);
            assert!(plan.phases[0].park_payload.is_none());
        }
    }

    #[test]
    fn budget_defaults_are_hard_and_named() {
        let plan = plan();
        assert_eq!(
            resolved_phase_token_cap(&plan),
            limits::DEFAULT_RUN_PHASE_TOKEN_CAP
        );
        assert_eq!(
            resolved_phase_timeout(&plan),
            Duration::from_secs(limits::DEFAULT_RUN_PHASE_WALL_CLOCK_SECS)
        );
        assert_eq!(
            resolved_run_timeout(&plan),
            Duration::from_secs(limits::DEFAULT_RUN_TOTAL_WALL_CLOCK_SECS)
        );
    }

    #[test]
    fn plan_overrides_replace_every_default_budget() {
        let mut plan = plan();
        plan.budgets = RunBudgets {
            per_phase_token_cap: Some(123),
            per_phase_wall_clock_secs: Some(45),
            total_run_wall_clock_secs: Some(67),
        };
        assert_eq!(resolved_phase_token_cap(&plan), 123);
        assert_eq!(resolved_phase_timeout(&plan), Duration::from_secs(45));
        assert_eq!(resolved_run_timeout(&plan), Duration::from_secs(67));
    }

    #[test]
    fn run_branch_name_is_safe_and_scoped() {
        assert_eq!(run_branch_name("run a/b"), "run/run-a-b");
    }

    #[tokio::test]
    async fn stuck_bootstrap_child_is_killed_by_timeout() {
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .kill_on_drop(true)
            .spawn()
            .expect("spawn stuck fixture");
        let cancel = AtomicBool::new(false);
        let outcome = wait_for_bootstrap_child(&mut child, Duration::from_millis(20), &cancel)
            .await
            .expect("watchdog should return a stop reason");
        assert_eq!(outcome, BootstrapStop::TimedOut);
        child.kill().await.expect("kill stuck fixture");
        child.wait().await.expect("reap stuck fixture");
    }

    /// The epic's top risk (`docs/postmortem-runaway-token-usage.md`): a
    /// phase turn that never finishes must not burn the night unnoticed. This
    /// races `race_with_watchdog` — the exact mechanism `execute_run` uses to
    /// decide "kill this phase" — against a deliberately stuck fixture future
    /// standing in for a wedged `claude_session` turn (no mockable seam for a
    /// real one exists in this codebase; see `run_executor.rs`'s test module
    /// docs for the same limitation on `execute_run` itself).
    #[tokio::test]
    async fn stuck_phase_turn_is_caught_by_the_watchdog() {
        let stuck = async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok::<u32, AppError>(0)
        };
        let outcome = race_with_watchdog(Duration::from_millis(20), stuck).await;
        assert!(matches!(outcome, WatchdogOutcome::TimedOut));
    }

    /// The non-stuck path: a turn that finishes well inside its budget is
    /// reported as `Completed` with its own result, not mistaken for a stall.
    #[tokio::test]
    async fn fast_phase_turn_completes_before_the_watchdog_fires() {
        let fast = async { Ok::<u32, AppError>(42) };
        let outcome = race_with_watchdog(Duration::from_secs(5), fast).await;
        match outcome {
            WatchdogOutcome::Completed(Ok(value)) => assert_eq!(value, 42),
            other => panic!("expected Completed(Ok(42)), got {other:?}"),
        }
    }

    /// A turn-level error (not a stall) still surfaces through `Completed`,
    /// not `TimedOut` — the watchdog and turn-failure paths must stay
    /// distinguishable so `execute_run` reports the right phase status
    /// (`Failed` vs `Killed`).
    #[tokio::test]
    async fn turn_error_within_budget_is_not_mistaken_for_a_timeout() {
        let erroring = async { Err::<u32, AppError>(AppError::RunPlan("boom".into())) };
        let outcome = race_with_watchdog(Duration::from_secs(5), erroring).await;
        match outcome {
            WatchdogOutcome::Completed(Err(AppError::RunPlan(msg))) => assert_eq!(msg, "boom"),
            other => panic!("expected Completed(Err(RunPlan(\"boom\"))), got {other:?}"),
        }
    }

    fn empty_app_state() -> AppState {
        AppState {
            config: std::sync::Mutex::new(crate::config::GlobalConfig::default()),
            claude_sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending_answers: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending_permissions: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending_plans: std::sync::Mutex::new(std::collections::HashMap::new()),
            interrupt_slots: std::sync::Mutex::new(std::collections::HashMap::new()),
            run_handles: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    // ── worktree lifecycle: create / resume / prune / keep-on-failure ──────

    fn init_test_repo(dir: &Path) {
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@loopdeck.dev"],
            vec!["config", "user.name", "LoopDeck Test"],
        ] {
            std::process::Command::new("git")
                .args(["-C"])
                .arg(dir)
                .args(args)
                .output()
                .unwrap();
        }
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        for args in [vec!["add", "."], vec!["commit", "-m", "initial"]] {
            std::process::Command::new("git")
                .args(["-C"])
                .arg(dir)
                .args(args)
                .output()
                .unwrap();
        }
    }

    /// A fresh temp git repo (with one commit) for worktree lifecycle tests.
    /// Distinct from `plan()` above, which targets a fake `/repo` path — these
    /// tests spawn real `git worktree` commands, so they need a real repo.
    fn temp_git_repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loopdeck-runqueue-worktree-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        init_test_repo(&dir);
        dir
    }

    fn plan_with_phases(project: &Path, phases: Vec<runplan::RunPhase>) -> RunPlan {
        let mut p = plan();
        // `run_worktree_path` derives the worktree's destination from
        // `root.parent()/.loopdeck-runs/<plan.id>` — every temp repo here
        // shares the OS temp dir as its parent, so a shared `plan.id` (the
        // `plan()` fixture's fixed "run-budget-test") would make parallel
        // tests race to create a linked worktree at the identical path. A
        // unique id per test keeps each worktree lifecycle test isolated.
        p.id = format!("run-{}", uuid::Uuid::new_v4());
        p.project = project.to_path_buf();
        p.phases = phases;
        p
    }

    fn completed_phase(id: &str) -> runplan::RunPhase {
        runplan::RunPhase {
            execution_id: id.into(),
            status: RunPhaseStatus::Completed,
            interview: vec![],
            interview_status: InterviewStatus::Answered,
            depends_on: vec![],
            park_payload: None,
            token_usage: 0,
            wall_clock_secs: 0,
        }
    }

    fn failed_phase(id: &str) -> runplan::RunPhase {
        runplan::RunPhase {
            status: RunPhaseStatus::Failed,
            ..completed_phase(id)
        }
    }

    /// Create: `ensure_worktree` on a plan with no recorded environment
    /// creates a real linked worktree and persists its path/branch.
    #[test]
    fn ensure_worktree_creates_and_persists_environment() {
        let dir = temp_git_repo("create");
        let mut plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);

        let worktree = ensure_worktree(&dir, &mut plan).expect("create worktree");

        assert_eq!(
            plan.environment.worktree_path.as_deref(),
            Some(worktree.as_path())
        );
        assert!(plan.environment.worktree_branch.is_some());
        assert!(git::worktree_list(&dir).unwrap().contains(&worktree));

        git::worktree_remove(&dir, &worktree).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Resume: calling `ensure_worktree` again on a plan whose recorded
    /// worktree still exists is idempotent — no second worktree/branch.
    #[test]
    fn ensure_worktree_resumes_an_existing_worktree_without_duplicating_it() {
        let dir = temp_git_repo("resume");
        let mut plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        let first = ensure_worktree(&dir, &mut plan).expect("create worktree");

        let second = ensure_worktree(&dir, &mut plan).expect("resume worktree");

        assert_eq!(first, second);
        assert_eq!(
            git::worktree_list(&dir)
                .unwrap()
                .iter()
                .filter(|p| **p == first)
                .count(),
            1,
            "resuming must not create a second linked worktree"
        );

        git::worktree_remove(&dir, &first).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A restart whose recorded worktree was deleted out-of-band (e.g. by
    /// hand, or a prior crash mid-remove) fails loudly rather than silently
    /// creating a second branch for the same run.
    #[test]
    fn ensure_worktree_errors_when_recorded_worktree_is_missing() {
        let dir = temp_git_repo("missing");
        let mut plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        plan.environment.worktree_path = Some(dir.join("nonexistent-linked-worktree"));

        let err = ensure_worktree(&dir, &mut plan).unwrap_err();
        assert!(err.to_string().contains("missing"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Prune: every phase `Completed` → the worktree is removed and the
    /// plan's environment fields are cleared.
    #[test]
    fn finalize_worktree_prunes_on_full_success() {
        let dir = temp_git_repo("prune");
        let mut plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        let worktree = ensure_worktree(&dir, &mut plan).expect("create worktree");

        finalize_worktree(&dir, &worktree, &mut plan);

        assert!(!plan.environment.worktree_kept);
        assert!(plan.environment.worktree_path.is_none());
        assert!(plan.environment.worktree_branch.is_none());
        assert!(!worktree.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Keep-on-failure: any phase not `Completed` → the worktree is left on
    /// disk and flagged, so a failed run stays inspectable.
    #[test]
    fn finalize_worktree_keeps_on_any_non_completed_phase() {
        let dir = temp_git_repo("keep-failed");
        let mut plan = plan_with_phases(
            &dir,
            vec![completed_phase("phase-1"), failed_phase("phase-2")],
        );
        let worktree = ensure_worktree(&dir, &mut plan).expect("create worktree");

        finalize_worktree(&dir, &worktree, &mut plan);

        assert!(plan.environment.worktree_kept);
        assert_eq!(
            plan.environment.worktree_path.as_deref(),
            Some(worktree.as_path())
        );
        assert!(worktree.exists(), "a kept worktree must remain on disk");

        git::worktree_remove(&dir, &worktree).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Keep-and-flag: every phase `Completed` but the prune itself fails
    /// (e.g. the recorded path no longer matches a real linked worktree) —
    /// the run must not lose track of it silently.
    #[test]
    fn finalize_worktree_keeps_and_flags_when_prune_fails() {
        let dir = temp_git_repo("prune-fails");
        let mut plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        // No real worktree was ever created at this path — `git worktree
        // remove` must fail.
        let bogus = dir.join("never-actually-a-worktree");

        finalize_worktree(&dir, &bogus, &mut plan);

        assert!(plan.environment.worktree_kept);
        let payload = plan.phases.last().unwrap().park_payload.as_deref();
        assert!(
            payload.is_some_and(|p| p.contains("worktree cleanup failed")),
            "expected a cleanup-failure payload, got {payload:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `terminate_session` is the side effect a caught stall relies on to
    /// actually free the stuck process (its removal drops the last `Arc`,
    /// which invokes the harness's graceful-EOF-then-SIGKILL `Drop`). Without
    /// a mockable `claude_session`, this only proves the map-eviction half —
    /// removing an absent entry is a safe no-op, which is what every timeout
    /// on a project with no cached session (the common unattended-run case:
    /// each phase spawns fresh) actually exercises.
    #[test]
    fn terminate_session_on_an_absent_entry_is_a_safe_no_op() {
        let state = empty_app_state();
        assert!(terminate_session(&state, Path::new("/does/not/matter")).is_ok());
    }
}
