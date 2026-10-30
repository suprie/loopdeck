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
    mark_turn_terminal, start_fresh_and_record_streaming_in_root_with_config,
    start_fresh_and_record_streaming_in_root_with_config_resume,
};
use super::state::{
    fire_interrupt, resolve_agent_config, resolve_agent_config_by_id, resolve_root, AppState,
};
use crate::agents::{AgentResponse, AskUserQuestionSpec, ClaudeEvent, TokenBudget};
use crate::config::AgentHarness;
use crate::delivery;
use crate::delivery_retry;
use crate::epic;
use crate::error::AppError;
use crate::execution::{self, ExecutionState, LoopOrigin};
use crate::git;
use crate::handoff;
use crate::limits;
use crate::provider_event::{RunEvent, RunEventEnvelope};
use crate::provider_normalize::RunEventChannel;
use crate::run_executor::{
    self, build_batch_interview_prompt, build_codex_batch_interview_prompt,
    build_codex_interview_prompt, build_combined_phase_prompt, build_interview_prompt,
    extract_batch_interview_answers, extract_draft_pr_url, extract_interview_answers,
    extract_verdict, ResolvedBudgets, RunHandle, RunVerdict,
};
use crate::runplan::{self, InterviewStatus, RunBudgets, RunPhaseStatus, RunPlan, StallPolicy};
use crate::scheduler;
use chrono::Utc;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Child;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tracing::{info, warn};
use uuid::Uuid;

const RUN_QUEUE_EVENT: &str = "run-queue-event";

/// Additive normalized event topic. The legacy `RUN_QUEUE_EVENT` above keeps
/// its exact `ClaudeEvent` payload; this stream carries the provider-neutral
/// `RunEvent` produced by the same adapter conversion.
const RUN_EVENT_TOPIC: &str = "run-event";

/// A `Channel<RunEvent>` whose sink re-broadcasts on the app-wide topic. The
/// detached executor cannot hold a frontend-owned IPC channel, so this
/// mirrors the legacy channel's re-emit trick.
fn normalized_run_channel(
    app: AppHandle,
    project_path: String,
    execution_id: String,
) -> Channel<RunEvent> {
    Channel::new(move |event| {
        let event = match event.deserialize::<RunEvent>() {
            Ok(event) => event,
            Err(error) => {
                warn!("failed to decode normalized run activity: {error}");
                return Ok(());
            }
        };
        if let Err(error) = app.emit(
            RUN_EVENT_TOPIC,
            RunEventEnvelope {
                project_path: project_path.clone(),
                execution_id: execution_id.clone(),
                event,
            },
        ) {
            warn!("failed to emit normalized run activity: {error}");
        }
        Ok(())
    })
}

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

/// Send an OS notification for a terminal run transition (prd-wake-up Phase 1).
/// Three call sites, per the PRD: run completed, budget kill, all phases parked.
/// On click, the OS activates the app; we focus the main window so the morning
/// report is the first thing the user sees.
fn notify_run_terminal(app: &AppHandle, root: &Path, title: &str, body: &str) {
    let project_name = root
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        warn!("failed to send OS notification for run \"{project_name}\": {e}");
    }
    // Focus the main window so clicking the notification lands on the report.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_focus();
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

/// Ensure a phase's own isolated worktree exists (`parallel-scheduling`).
///
/// Every phase gets one worktree and one branch, cut from the repository's
/// default branch — no stacking. Dependents receive upstream output only via
/// handoff artifacts, never via an upstream branch's unmerged code (the
/// approved branch-scoping answer). One worktree per in-flight phase also
/// keeps the session cache honest: `AppState.claude_sessions` is keyed by
/// worktree path, so concurrent turns can never share a provider process.
///
/// Resume behavior (`prd-verified-delivery-reconciliation` Phase 2): when the
/// persisted worktree is gone from Git's inventory but its **branch still
/// exists**, the worktree is recreated at the same path from the surviving
/// branch instead of hard-stopping. Only a missing branch preserves the run
/// for inspection.
///
/// Returns `(path, branch, created)`; the caller persists both fields on the
/// phase before spawning the turn.
fn ensure_phase_worktree(
    root: &Path,
    plan: &RunPlan,
    index: usize,
) -> Result<(PathBuf, String, bool), AppError> {
    let phase = &plan.phases[index];
    let branch = phase
        .worktree_branch
        .clone()
        .unwrap_or_else(|| scheduler::phase_branch_name(&plan.id, &phase.execution_id, index));
    let path = phase
        .worktree_path
        .clone()
        .unwrap_or_else(|| scheduler::phase_worktree_path(root, &branch));

    let known = git::worktree_list(root).map_err(AppError::RunPlan)?;
    if known.iter().any(|candidate| candidate == &path) {
        return Ok((path, branch, false));
    }
    if git::branch_exists(root, &branch) {
        let worktree =
            git::worktree_add_existing(root, &path, &branch).map_err(AppError::RunPlan)?;
        info!(
            "recreated phase worktree {} from surviving branch {branch}",
            worktree.path.display()
        );
        return Ok((worktree.path, worktree.branch, true));
    }

    ignore_managed_runs_dir(root);
    std::fs::create_dir_all(path.parent().expect("worktree path has a parent"))?;
    let base = git::default_branch(root).unwrap_or_else(|| "HEAD".to_string());
    let worktree =
        git::worktree_add_from(root, &path, &branch, &base).map_err(AppError::RunPlan)?;
    Ok((worktree.path, worktree.branch, true))
}

/// Persist the clean-handoff record at delivery (Phase 4, `clean-handoff`).
/// Lazy by design: this records *what was delivered and where the next run
/// starts from* — the `.loopdeck/runs/<next-branch>/` worktree is only created
/// when the next loop actually starts. Best-effort: a failed write parks a
/// note on the last phase, it never fails the delivery itself.
fn record_handoff(root: &Path, branch: &str, worktree: &Path, pr_url: &str) {
    let record = handoff::HandoffRecord {
        delivered_branch: branch.to_string(),
        pr_url: pr_url.to_string(),
        worktree: worktree.to_path_buf(),
        next_base: git::default_branch(root).unwrap_or_else(|| "main".to_string()),
        delivered_at: chrono::Utc::now(),
    };
    if let Err(error) = handoff::save(root, &record) {
        warn!("could not persist delivery handoff record: {error}");
    }
}

/// Promote one phase's agent-authored artifact from its worktree into the
/// project's persistent handoff store. Validation and duplicate detection
/// live in `handoff`; a missing or mismatched artifact errors so the executor
/// can park the phase without losing the original.
fn emit_phase_artifact(
    root: &Path,
    worktree: &Path,
    execution_id: &str,
    topic: Option<&str>,
) -> Result<(), AppError> {
    // Handoffs are opt-in per phase (`prd-agent-handoff` Phase 1): a phase
    // without a configured topic has no artifact contract, so legacy plans
    // keep executing unchanged (ADR-3).
    let Some(topic) = topic else {
        return Ok(());
    };
    handoff::copy_agent_artifact(root, worktree, execution_id, topic)?;
    Ok(())
}

/// Managed run worktrees live inside the repo (`.loopdeck/runs/`), so without
/// an ignore rule every run shows up as untracked noise in the main worktree.
/// Idempotently append the rule to `.gitignore` — the only file this touches.
fn ignore_managed_runs_dir(root: &Path) {
    const RULE: &str = ".loopdeck/runs/";
    let gitignore = root.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == RULE) {
        return;
    }
    let mut next = existing;
    if !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(RULE);
    next.push('\n');
    if std::fs::write(&gitignore, next).is_err() {
        warn!("could not add {RULE} to .gitignore — runs will show as untracked");
    }
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
    info!(
        target: "loopdeck::run_queue",
        path = %path.display(),
        had_session = session.is_some(),
        "run queue removed cached agent session"
    );
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

/// Apply queue-time handoff topics to a freshly built plan
/// (`prd-agent-handoff` Phase 1). One artifact = one topic = one file, so a
/// topic must be a kebab-case slug, belong to a phase in this plan, and be
/// unique both per phase and across the plan — a duplicate would otherwise
/// park the second phase at emission time ("original preserved"). The slug is
/// always supplied by the caller; the executor never derives it from the
/// execution id.
fn apply_handoff_topics(
    plan: &mut RunPlan,
    execution_ids: &[String],
    handoff_topics: Vec<runplan::PhaseHandoffTopic>,
) -> Result<(), AppError> {
    let mut configured_phases = HashSet::new();
    let mut configured_topics = HashSet::new();
    for configured in handoff_topics {
        if !execution_ids.contains(&configured.execution_id) {
            return Err(AppError::RunPlan(format!(
                "handoff topic references phase \"{}\", which is not part of this plan",
                configured.execution_id
            )));
        }
        if configured.topic.is_empty()
            || handoff::topic_for_phase(&configured.topic) != configured.topic
            || !configured_phases.insert(configured.execution_id.clone())
            || !configured_topics.insert(configured.topic.clone())
        {
            return Err(AppError::RunPlan(
                "handoff topics must be unique kebab-case slugs configured once per phase".into(),
            ));
        }
        let phase = plan
            .phases
            .iter_mut()
            .find(|phase| phase.execution_id == configured.execution_id)
            .expect("validated phase id");
        phase.handoff_topic = Some(configured.topic);
    }
    Ok(())
}

/// Build and persist a new `.loopdeck/run-plan.yaml` from a picker selection
/// (Phase 5): the phases the user checked, in the order they were selected,
/// under one queue-time `stall_policy` and draft-PR consent.
///
/// Every ID is validated against `docs/epics/` (`epic::find_loop_by_id`) so a
/// stale or mistyped ID is rejected before it's written to disk, not
/// discovered later at run time. Each phase's `assigned_agent` (when the
/// picker set one) is validated against the agent roster the same way, so a
/// deleted profile is caught at queue time — the executor's missing-agent
/// park is only the overnight safety net for entries removed between plan
/// creation and execution. `depends_on` defaults to the authored
/// selection order — each phase depends on its immediate predecessor — per
/// the PRD's "linear chain, no editor" v1 design; there is no edge editor in
/// this phase. Every phase starts `Queued`/`Pending` (interview unanswered),
/// so `queue_run` still gates on the pre-flight interview as usual.
///
/// Replaces any existing plan outright — a fresh picker selection is always
/// meant to start a new plan, not merge into a stale one — but refuses while
/// a run is actively executing (same guard `queue_run` uses), so a picker
/// selection can't clobber the plan an in-flight executor is reading.
// Tauri commands receive their arguments as a flat IPC object, so the queue
// request's independent knobs stay top-level rather than nesting new structs.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn create_run_plan(
    path: String,
    execution_ids: Vec<String>,
    stall_policy: StallPolicy,
    draft_pr_authorized: bool,
    budgets: RunBudgets,
    // Per-phase agent staffing (`prd-role-foundations` Phase 4). Optional
    // and sparse — phases without an entry stay on the default agent.
    assignments: Option<Vec<runplan::PhaseAgentAssignment>>,
    handoff_topics: Option<Vec<runplan::PhaseHandoffTopic>>,
    // Maximum phases that may execute at once (`prd-parallel-scheduling`
    // Phase 2). `None` keeps the four-phase default; the picker may set any
    // value in 1..=MAX_RUN_MAX_CONCURRENCY.
    max_concurrency: Option<u32>,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;
    validate_budgets(&budgets)?;
    let max_concurrency = max_concurrency.unwrap_or(limits::DEFAULT_RUN_MAX_CONCURRENCY);
    if max_concurrency == 0 || max_concurrency > limits::MAX_RUN_MAX_CONCURRENCY {
        return Err(AppError::RunPlan(format!(
            "max concurrency must be between 1 and {} phases",
            limits::MAX_RUN_MAX_CONCURRENCY
        )));
    }

    if execution_ids.is_empty() {
        return Err(AppError::RunPlan(
            "select at least one phase to queue".into(),
        ));
    }
    if execution_ids.iter().collect::<HashSet<_>>().len() != execution_ids.len() {
        return Err(AppError::RunPlan(
            "a phase can only appear once in a run plan".into(),
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

    // Resolve staffing against the roster before persisting: an unknown
    // execution_id is a picker/UI bug, an unknown agent_id is a stale roster
    // reference — both must fail the create, never queue a phase that can
    // never spawn. The roster's own `name` is captured here so attribution in
    // the run report is durable (survives later renames/deletes).
    let assignments = assignments.unwrap_or_default();
    let mut resolved_assignments = Vec::with_capacity(assignments.len());
    {
        let config = state.config.lock().map_err(|_| AppError::LockError)?;
        for assignment in &assignments {
            if !execution_ids.contains(&assignment.execution_id) {
                return Err(AppError::RunPlan(format!(
                    "assignment references phase \"{}\", which is not part of this plan",
                    assignment.execution_id
                )));
            }
            let named = config
                .find_agent_config(&assignment.agent_id)
                .ok_or_else(|| {
                    AppError::RunPlan(format!(
                        "assigned agent '{}' was not found on the roster",
                        assignment.agent_id
                    ))
                })?;
            resolved_assignments.push(runplan::PhaseAgentAssignment {
                execution_id: assignment.execution_id.clone(),
                agent_id: assignment.agent_id.clone(),
                agent_name: Some(named.name.clone()),
            });
        }
    }

    if let Some(existing) = runplan::load(&root)? {
        for phase in &existing.phases {
            let has_draft_pr = phase
                .park_payload
                .as_deref()
                .is_some_and(|payload| payload.starts_with("draft PR: https://github.com/"));
            if has_draft_pr
                && execution_ids.iter().any(|id| id == &phase.execution_id)
                && matches!(
                    phase.status,
                    RunPhaseStatus::Completed | RunPhaseStatus::Delivered
                )
            {
                return Err(AppError::RunPlan(format!(
                    "phase \"{}\" already has recorded draft-PR delivery; relink or review that PR instead of rerunning it",
                    phase.execution_id
                )));
            }
        }
    }

    let mut plan = run_executor::build_run_plan(
        format!("run-{}", Uuid::new_v4()),
        root.clone(),
        Utc::now(),
        &execution_ids,
        stall_policy,
        draft_pr_authorized,
        &resolved_assignments,
    );
    apply_handoff_topics(
        &mut plan,
        &execution_ids,
        handoff_topics.unwrap_or_default(),
    )?;
    plan.budgets = budgets;
    plan.max_concurrency = max_concurrency;
    // Reject a cycle before it is persisted: a plan with one would hold every
    // member Waiting forever when the run starts.
    scheduler::validate_dependency_graph(&plan.phases)?;

    runplan::save(&root, &plan)?;
    info!(
        "run plan queued for {} with {} phase(s), max concurrency {}",
        root.display(),
        plan.phases.len(),
        plan.max_concurrency
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
        .any(|p| matches!(p.status, RunPhaseStatus::Queued | RunPhaseStatus::Waiting))
    {
        return Err(AppError::RunPlan(
            "the run plan has no queued phases".into(),
        ));
    }

    if let Some(phase) = plan.phases.iter().find(|p| {
        matches!(p.status, RunPhaseStatus::Queued | RunPhaseStatus::Waiting)
            && p.interview_status == InterviewStatus::Pending
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

    // Phase sessions run in their own isolated worktrees, while legacy/manual
    // sessions are keyed by the registered root. Signal every active phase
    // worktree and the legacy plan-level worktree so Stop reaches all
    // concurrent turns immediately.
    let _ = fire_interrupt(&state, &root)?;
    if let Some(plan) = runplan::load(&root)? {
        if let Some(worktree) = plan.environment.worktree_path.as_ref() {
            let _ = fire_interrupt(&state, worktree)?;
        }
        for phase in plan.phases.iter().filter(|phase| {
            scheduler::is_active(phase.status) || phase.status == RunPhaseStatus::Running
        }) {
            if let Some(worktree) = phase.worktree_path.as_ref() {
                let _ = fire_interrupt(&state, worktree)?;
            }
        }
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

/// Morning report read model — joins the on-disk run plan with derived
/// per-phase verdict labels and the overnight audit slice (prd-wake-up Phase 2).
/// Pure read: no side effects, no new storage.
#[tauri::command]
pub async fn get_run_report(
    path: String,
    state: State<'_, AppState>,
) -> Result<crate::runplan::RunReport, AppError> {
    let root = resolve_root(&state, &path)?;
    let plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan is available for this project".into()))?;
    let audit = build_audit_slice(&root, &plan).unwrap_or_default();
    Ok(crate::runplan::RunReport::from_plan(plan, audit))
}

/// Answer a parked phase's `AskUserQuestion`, pin the answers into its
/// interview, and requeue it (prd-wake-up Phase 2). The phase must be `Parked`
/// and its `park_payload` must carry a `__QUESTIONS__` marker (meaning the
/// turn parked on an unanswered question, not a verify-verdict park).
#[tauri::command]
pub async fn answer_parked_question(
    path: String,
    execution_id: String,
    answers: std::collections::HashMap<String, super::agent::AnswerWire>,
    state: State<'_, AppState>,
) -> Result<crate::runplan::RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;
    let mut plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan is available for this project".into()))?;

    let phase = plan
        .phases
        .iter_mut()
        .find(|p| p.execution_id == execution_id && p.status == RunPhaseStatus::Parked)
        .ok_or_else(|| {
            AppError::RunPlan(format!(
                "phase \"{execution_id}\" is not parked or does not exist"
            ))
        })?;

    // Pin each answer as a PinnedAnswer. Labels are joined with ", "; free-text
    // "Other" answers use the raw text. Matches the answer format the existing
    // `agent_answer_question` uses for live turns.
    for (question, answer) in &answers {
        let text = if !answer.other_text.as_deref().unwrap_or("").trim().is_empty() {
            answer.other_text.clone().unwrap_or_default()
        } else {
            answer.labels.join(", ")
        };
        if !text.is_empty() {
            phase.interview.push(crate::runplan::PinnedAnswer {
                question: question.clone(),
                answer: text,
            });
        }
    }
    phase.interview_status = InterviewStatus::Answered;
    phase.status = RunPhaseStatus::Queued;
    // Clear the park payload now that the question has been answered.
    phase.park_payload = None;

    runplan::save(&root, &plan)?;
    Ok(plan)
}

/// Build the overnight audit slice from the project's tracing logs.
/// Scans log lines within the run window for `permission decision` events
/// emitted by the autonomous policy layer. Log-format dependent — best-effort.
fn build_audit_slice(
    _repo_path: &Path,
    plan: &crate::runplan::RunPlan,
) -> Result<crate::runplan::AuditSlice, AppError> {
    let Some(log_dir) = crate::logging::log_dir() else {
        return Ok(crate::runplan::AuditSlice::default());
    };
    let run_start = plan.created;
    let run_end = if plan.wall_clock_secs > 0 {
        run_start + chrono::Duration::seconds(plan.wall_clock_secs as i64)
    } else {
        chrono::Utc::now()
    };

    let mut auto_allow_count: u64 = 0;
    let mut floor_denials: Vec<String> = Vec::new();

    // Walk each date from run_start to run_end, reading that day's log file.
    let mut day = run_start.date_naive();
    let end_day = run_end.date_naive();
    while day <= end_day {
        let filename = format!("loopdeck.log.{}", day.format("%Y-%m-%d"));
        let path = log_dir.join(&filename);
        if path.exists() {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                for line in contents.lines() {
                    if !line.contains("permission decision") {
                        continue;
                    }
                    // Parse the ISO timestamp at the start of the line to filter
                    // by the run window. Format: "2026-08-04T01:23:45.678Z"
                    if let Some(ts_end) = line.find("Z ") {
                        let ts_str = &line[..=ts_end];
                        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(ts_str) {
                            let ts = ts.with_timezone(&chrono::Utc);
                            if ts < run_start || ts > run_end {
                                continue;
                            }
                        }
                    }
                    // Count autonomous auto-allows.
                    if line.contains("autonomous=true") {
                        auto_allow_count += 1;
                    }
                    // Collect floor denials.
                    if line.contains("behavior=deny") {
                        // Extract tool name and reason for the itemized list.
                        let tool = extract_field(line, "tool").unwrap_or_else(|| "unknown".into());
                        let reason = extract_field(line, "reason").unwrap_or_else(|| "".into());
                        let input = extract_field(line, "input").unwrap_or_else(|| "".into());
                        let summary = if reason.is_empty() {
                            format!("{tool}: {input}")
                        } else {
                            format!("{tool}: {input} — {reason}")
                        };
                        floor_denials.push(truncate(&summary, 200));
                    }
                }
            }
        }
        day = day.succ_opt().unwrap_or(end_day);
    }

    Ok(crate::runplan::AuditSlice {
        auto_allow_count,
        floor_denials,
    })
}

/// Extract a `key="value"` or `key=value` field from a tracing log line.
fn extract_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"')?;
        Some(inner[..end].to_string())
    } else {
        let end = rest.find(' ').unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
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
        RunPhaseStatus::Completed | RunPhaseStatus::Delivered => {
            state.complete_current(now, None, false)?
        }
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
        // A `Queued` phase that restart reconciliation auto-resumed is
        // restart residue: release the stale focus pointer so the next queue
        // attempt is not blocked by a loop nothing is driving. A plain queued
        // phase that merely matches a manually promoted current loop keeps
        // it — the user is still working that loop.
        RunPhaseStatus::Queued if phase.resume_session_id.is_some() => state.abandon_current(
            format!(
                "reconciled auto-resumed run phase \"{}\" left queued after restart",
                phase.execution_id
            ),
            now,
            false,
        )?,
        RunPhaseStatus::Queued | RunPhaseStatus::Waiting | RunPhaseStatus::Running => {
            return Ok(None)
        }
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

#[tauri::command]
pub async fn relink_delivered_phase(
    path: String,
    execution_id: String,
    replacement_id: String,
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
            "cannot relink a phase while the run is still active".into(),
        ));
    }
    if execution_id == replacement_id {
        return Err(AppError::RunPlan(
            "choose a different PRD item to relink".into(),
        ));
    }
    let replacement = epic::find_loop_by_id(&root, &replacement_id).ok_or_else(|| {
        AppError::RunPlan(format!(
            "no PRD checklist loop with id \"{replacement_id}\" found under docs/epics/"
        ))
    })?;

    let mut plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan exists for this project".into()))?;
    let phase_index = plan
        .phases
        .iter()
        .position(|phase| phase.execution_id == execution_id)
        .ok_or_else(|| {
            AppError::RunPlan(format!("phase \"{execution_id}\" is not part of this run"))
        })?;
    if plan.phases[phase_index].status != RunPhaseStatus::Delivered {
        return Err(AppError::RunPlan(format!(
            "phase \"{execution_id}\" is not awaiting PRD relinking"
        )));
    }
    if plan
        .phases
        .iter()
        .any(|phase| phase.execution_id == replacement_id)
    {
        return Err(AppError::RunPlan(format!(
            "PRD item \"{replacement_id}\" already belongs to this run"
        )));
    }

    let loaded = execution::load(&root)?;
    let revision = loaded.state.revision;
    let mut execution_state = loaded.state;
    if execution_state
        .current
        .as_ref()
        .is_some_and(|current| current.id == replacement_id)
        || execution_state
            .queue
            .iter()
            .any(|queued| queued.id == replacement_id)
        || execution_state
            .history
            .iter()
            .any(|history| history.id == replacement_id)
    {
        return Err(AppError::RunPlan(format!(
            "PRD item \"{replacement_id}\" already has execution history"
        )));
    }
    let mut relinked_execution = false;
    if let Some(current) = execution_state
        .current
        .as_mut()
        .filter(|current| current.id == execution_id)
    {
        current.id = replacement_id.clone();
        current.title = replacement.title.clone();
        current.origin = LoopOrigin {
            epic: replacement.epic.clone(),
            prd: replacement.prd.clone(),
            phase: replacement.phase.clone(),
        };
        relinked_execution = true;
    }
    for queued in execution_state
        .queue
        .iter_mut()
        .filter(|queued| queued.id == execution_id)
    {
        queued.id = replacement_id.clone();
        queued.title = replacement.title.clone();
        queued.origin = LoopOrigin {
            epic: replacement.epic.clone(),
            prd: replacement.prd.clone(),
            phase: replacement.phase.clone(),
        };
        relinked_execution = true;
    }
    for history in execution_state
        .history
        .iter_mut()
        .filter(|history| history.id == execution_id)
    {
        history.id = replacement_id.clone();
        history.title = replacement.title.clone();
        history.origin = LoopOrigin {
            epic: replacement.epic.clone(),
            prd: replacement.prd.clone(),
            phase: replacement.phase.clone(),
        };
        relinked_execution = true;
    }
    if relinked_execution {
        execution_state.validate()?;
    }

    plan.phases[phase_index].execution_id = replacement_id.clone();
    plan.phases[phase_index].status = RunPhaseStatus::Completed;
    for phase in &mut plan.phases {
        for dependency in &mut phase.depends_on {
            if *dependency == execution_id {
                *dependency = replacement_id.clone();
            }
        }
    }
    if relinked_execution {
        execution::save(&root, &execution_state, revision)?;
    }
    runplan::save(&root, &plan)?;
    Ok(plan)
}

/// Requeue every retryable terminal phase at once — the mass-retry path for
/// a run that died wholesale (e.g. a bootstrap failure that killed the whole
/// batch). Unlike [`requeue_run_phase`], no dependents cascade is needed:
/// every terminal phase is requeued regardless of why it stopped, so the
/// next `queue_run` picks them all up as one combined turn (single session,
/// one verify→ship) instead of N per-phase re-reads.
#[tauri::command]
pub async fn requeue_failed_run_phases(
    path: String,
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
            "cannot retry phases while the run is still active".into(),
        ));
    }

    let mut plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan exists for this project".into()))?;
    let requeued = requeue_all_terminal_phases(&mut plan);
    if requeued == 0 {
        return Err(AppError::RunPlan(
            "no retryable phases — nothing is parked, failed, interrupted, or killed".into(),
        ));
    }
    runplan::save(&root, &plan)?;
    info!("requeued {requeued} terminal phase(s) for mass retry");
    Ok(plan)
}

pub(crate) fn requeue_terminal_phase(
    plan: &mut RunPlan,
    execution_id: &str,
) -> Result<(), AppError> {
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
            // Dependents fail (not park) under the parallel scheduler; accept
            // both markers so a retry of the upstream clears the cascade.
            let blocked_by_requeued_phase =
                matches!(
                    phase.status,
                    RunPhaseStatus::Parked | RunPhaseStatus::Failed
                ) && phase.park_payload.as_deref().is_some_and(|payload| {
                    payload.starts_with("blocked: depends on parked phase")
                        || payload.starts_with("failed: upstream phase")
                }) && phase.depends_on.iter().any(|id| requeued.contains(id));
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

/// Requeue every retryable terminal phase (`Parked`/`Failed`/`Interrupted`/
/// `Killed`), clearing its park payload. Returns how many phases were
/// requeued — 0 means the plan had nothing to retry. `Completed`/`Queued`/
/// `Running` phases are left untouched.
fn requeue_all_terminal_phases(plan: &mut RunPlan) -> usize {
    let mut requeued = 0;
    for phase in &mut plan.phases {
        if matches!(
            phase.status,
            RunPhaseStatus::Parked
                | RunPhaseStatus::Failed
                | RunPhaseStatus::Interrupted
                | RunPhaseStatus::Killed
        ) {
            phase.status = RunPhaseStatus::Queued;
            phase.park_payload = None;
            requeued += 1;
        }
    }
    requeued
}
/// Run one queued phase's pre-flight interview turn (Phase 3) — a bounded
/// session driven through the *streaming* pipeline
/// (`start_fresh_and_record_streaming`, no-op sink `Channel`, same trick
/// Phase 4's executor uses). This is load-bearing, not cosmetic: per
/// `claude_session.rs::answer_ask_user_question`'s own doc comment, a
/// user-input request on the *non*-streaming path (`channel: None`) has no UI
/// surface to answer from and is auto-denied immediately instead of parking —
/// exactly the request the harness-specific interview prompt tells the agent to make. A
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
    let assigned_agent_id = plan
        .phases
        .iter()
        .find(|p| p.execution_id == execution_id)
        .ok_or_else(|| {
            AppError::RunPlan(format!("phase \"{execution_id}\" is not in the run plan"))
        })?
        // The interview runs with the phase's staffed agent (Phase 4), so
        // pinned answers come from the role that will act on them.
        .assigned_agent_id
        .clone();

    let loc = epic::find_loop_by_id(&root, &execution_id).ok_or_else(|| {
        AppError::RunPlan(format!(
            "queued phase \"{execution_id}\" no longer exists in docs/epics/"
        ))
    })?;

    let prompt = match resolve_agent_config(&state)?.harness {
        AgentHarness::Claude => build_interview_prompt(&execution_id, &loc),
        AgentHarness::Codex => build_codex_interview_prompt(&execution_id, &loc),
    };
    // No-op sink: nothing here needs to narrate turn events to a UI channel —
    // only the channel's *presence* matters, so a parked user question is
    // answerable instead of auto-denied (see this fn's doc comment).
    let channel: Channel<ClaudeEvent> = Channel::new(|_| Ok(()));
    let agent_config = match assigned_agent_id.as_deref() {
        Some(agent_id) => resolve_agent_config_by_id(&state, agent_id)?,
        None => resolve_agent_config(&state)?,
    };
    let response = start_fresh_and_record_streaming_in_root_with_config(
        &state,
        &root,
        &root,
        &prompt,
        Some(loc.title.clone()),
        &channel,
        None,
        None,
        Some(&agent_config),
        None,
        false,
        false,
    )
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
    let unavailable_questions = unavailable_codex_questions(&answers);
    if !unavailable_questions.is_empty() {
        let payload = preflight_question_payload(&unavailable_questions)?;
        let phase = &mut plan.phases[idx];
        phase.status = RunPhaseStatus::Parked;
        phase.park_payload = Some(payload);
        runplan::save(&root, &plan)?;
        return Ok(plan);
    }
    plan.phases[idx].interview = answers;
    plan.phases[idx].interview_status = InterviewStatus::Answered;
    runplan::save(&root, &plan)?;

    info!(
        "interview answered for phase \"{execution_id}\" in {}",
        root.display()
    );
    Ok(plan)
}

/// Run one shared pre-flight interview for several queued phases. The agent
/// receives every selected phase's context in one turn and asks all necessary
/// questions through the single pending-question slot, avoiding one tap per
/// phase while preserving phase-specific pinned answers.
#[tauri::command]
pub async fn run_batch_phase_interviews(
    path: String,
    execution_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    if execution_ids.is_empty() {
        return Err(AppError::RunPlan(
            "choose at least one phase to interview".into(),
        ));
    }

    let root = resolve_root(&state, &path)?;
    let plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan is queued for this project".into()))?;
    // Preserve picker order in the shared context; a duplicate ID must not
    // cause an extra phase heading or a second state mutation.
    let mut ids = Vec::with_capacity(execution_ids.len());
    for execution_id in execution_ids {
        if !ids.contains(&execution_id) {
            ids.push(execution_id);
        }
    }
    let mut phases = Vec::with_capacity(ids.len());
    for execution_id in &ids {
        let phase = plan
            .phases
            .iter()
            .find(|phase| phase.execution_id == *execution_id)
            .ok_or_else(|| {
                AppError::RunPlan(format!("phase \"{execution_id}\" is not in the run plan"))
            })?;
        if phase.status != RunPhaseStatus::Queued
            || phase.interview_status != InterviewStatus::Pending
        {
            return Err(AppError::RunPlan(format!(
                "phase \"{execution_id}\" no longer needs a pre-flight interview"
            )));
        }
        let loc = epic::find_loop_by_id(&root, execution_id).ok_or_else(|| {
            AppError::RunPlan(format!(
                "queued phase \"{execution_id}\" no longer exists in docs/epics/"
            ))
        })?;
        phases.push((execution_id.clone(), loc));
    }

    let prompt = match resolve_agent_config(&state)?.harness {
        AgentHarness::Claude => build_batch_interview_prompt(&phases),
        AgentHarness::Codex => build_codex_batch_interview_prompt(&phases),
    };
    let channel: Channel<ClaudeEvent> = Channel::new(|_| Ok(()));
    // One shared turn, so one agent: the lead phase's staffing (Phase 4).
    // Mixed-assignment batches keep the first phase's role; the per-phase
    // interviews remain available for role-specific questioning.
    let lead_agent_id = plan
        .phases
        .iter()
        .find(|phase| phase.execution_id == ids[0])
        .and_then(|phase| phase.assigned_agent_id.clone());
    let agent_config = match lead_agent_id.as_deref() {
        Some(agent_id) => resolve_agent_config_by_id(&state, agent_id)?,
        None => resolve_agent_config(&state)?,
    };
    let response = start_fresh_and_record_streaming_in_root_with_config(
        &state,
        &root,
        &root,
        &prompt,
        Some(format!("Pre-flight interview ({} phases)", phases.len())),
        &channel,
        None,
        None,
        Some(&agent_config),
        None,
        false,
        false,
    )
    .await?;
    let answers_by_phase = extract_batch_interview_answers(&response.result);

    let mut plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("run plan disappeared during interview".into()))?;
    for execution_id in ids {
        let phase = plan
            .phases
            .iter_mut()
            .find(|phase| phase.execution_id == execution_id)
            .ok_or_else(|| {
                AppError::RunPlan(format!(
                    "phase \"{execution_id}\" was removed from the run plan during its interview"
                ))
            })?;
        let answers = answers_by_phase
            .get(&execution_id)
            .cloned()
            .unwrap_or_default();
        let unavailable_questions = unavailable_codex_questions(&answers);
        if unavailable_questions.is_empty() {
            phase.interview = answers;
            phase.interview_status = InterviewStatus::Answered;
        } else {
            phase.status = RunPhaseStatus::Parked;
            phase.park_payload = Some(preflight_question_payload(&unavailable_questions)?);
        }
    }
    runplan::save(&root, &plan)?;
    Ok(plan)
}

/// A Codex model may report that its native input request is unavailable in
/// the current collaboration mode. That is not an answer: preserve the
/// question as a standard parked card, where the user can answer it and
/// requeue the phase without losing the pre-flight decision.
fn unavailable_codex_questions(
    answers: &[crate::runplan::PinnedAnswer],
) -> Vec<AskUserQuestionSpec> {
    answers
        .iter()
        .filter(|answer| {
            let text = answer.answer.to_ascii_lowercase();
            text.contains("native input request unavailable")
                || text.contains("askuserquestion unavailable")
        })
        .map(|answer| AskUserQuestionSpec {
            question: answer.question.clone(),
            header: "Pre-flight question".into(),
            // The card's built-in Other field accepts the user's free-text
            // response when Codex could not supply selectable options.
            options: Vec::new(),
            multi_select: false,
        })
        .collect()
}

fn preflight_question_payload(questions: &[AskUserQuestionSpec]) -> Result<String, AppError> {
    let encoded = serde_json::to_string(questions).map_err(|error| {
        AppError::RunPlan(format!("failed to encode pre-flight question: {error}"))
    })?;
    Ok(format!(
        "__QUESTIONS__{encoded}__END__ Codex needs your pre-flight answer before this phase can run."
    ))
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
/// One phase's turn, prepared by the scheduler and moved into its own
/// Tokio task. Everything is owned so the future is `'static`; the plan
/// itself is never moved into a task — only the main scheduler loop mutates
/// and persists it, which is what keeps concurrent phase bookkeeping safe.
struct PhaseTask {
    index: usize,
    execution_id: String,
    worktree: PathBuf,
    branch: String,
    loc: epic::LoopLocation,
    prompt: String,
    agent_config: crate::config::AgentConfig,
    title: String,
    channel: Channel<ClaudeEvent>,
    run_events: Option<RunEventChannel>,
    token_cap: u64,
    phase_timeout: Duration,
    run_timeout: Duration,
    resume_session_id: Option<String>,
}

struct PhaseTurnOutcome {
    index: usize,
    execution_id: String,
    worktree: PathBuf,
    branch: String,
    loc: epic::LoopLocation,
    elapsed: Duration,
    tokens: u64,
    exceeded: bool,
    exceeded_message: String,
    result: Result<AgentResponse, AppError>,
}

/// Run one phase turn in its own task: bootstrap the phase worktree, spawn
/// the provider turn under this phase's own token and wall-clock budget, and
/// return everything the scheduler needs to record the outcome. Never
/// touches `RunPlan`.
async fn run_phase_turn(
    state: &AppState,
    root: &Path,
    cancel: &AtomicBool,
    task: PhaseTask,
) -> PhaseTurnOutcome {
    let started = Instant::now();
    let run_remaining = task.run_timeout;
    let (bootstrap_timeout, bootstrap_reason) =
        scheduler::phase_watchdog(task.phase_timeout, run_remaining);
    let bootstrapped = bootstrap_worktree(&task.worktree, bootstrap_timeout, cancel).await;
    let outcome = match bootstrapped {
        Ok(BootstrapStop::Completed) => {
            let token_budget = TokenBudget::new(task.token_cap);
            let run_events = task.run_events.clone();
            let turn = start_fresh_and_record_streaming_in_root_with_config_resume(
                state,
                &task.worktree,
                root,
                &task.prompt,
                Some(task.title.clone()),
                &task.channel,
                Some(&token_budget),
                run_events,
                Some(&task.agent_config),
                // Per-phase control key: sibling phases must never collide on
                // pending-question, permission, plan, or interrupt slots.
                Some(&task.worktree),
                true,
                false,
                task.resume_session_id.as_deref(),
            );
            let (watchdog_timeout, watchdog_reason) = scheduler::phase_watchdog(
                task.phase_timeout,
                run_remaining.saturating_sub(started.elapsed()),
            );
            info!(
                target: "loopdeck::run_queue",
                execution_id = %task.execution_id,
                worktree = %task.worktree.display(),
                watchdog_secs = watchdog_timeout.as_secs(),
                watchdog_reason,
                "run queue starting phase turn"
            );
            let result = match race_with_watchdog(watchdog_timeout, turn).await {
                WatchdogOutcome::Completed(result) => result,
                WatchdogOutcome::TimedOut => {
                    let _ = terminate_session(state, &task.worktree);
                    let error = AppError::RunPlan(format!(
                        "{watchdog_reason} after {} seconds",
                        watchdog_timeout.as_secs()
                    ));
                    mark_turn_terminal(root, &error);
                    Err(error)
                }
            };
            (result, token_budget)
        }
        Ok(BootstrapStop::Cancelled) => (
            Err(AppError::RunPlan(
                "run cancelled during worktree bootstrap".into(),
            )),
            TokenBudget::new(task.token_cap),
        ),
        Ok(BootstrapStop::TimedOut) => (
            Err(AppError::RunPlan(format!(
                "{bootstrap_reason} during worktree bootstrap after {} seconds",
                bootstrap_timeout.as_secs()
            ))),
            TokenBudget::new(task.token_cap),
        ),
        Err(error) => (Err(error), TokenBudget::new(task.token_cap)),
    };
    let (result, token_budget) = outcome;
    let mut tokens = token_budget.used();
    let mut exceeded = false;
    if let Ok(response) = &result {
        let observed = response
            .usage
            .as_ref()
            .map(|usage| usage.input_tokens.saturating_add(usage.output_tokens))
            .unwrap_or_default();
        tokens = tokens.max(observed);
        exceeded = token_budget.observe_total(observed);
    }
    PhaseTurnOutcome {
        index: task.index,
        execution_id: task.execution_id,
        worktree: task.worktree,
        branch: task.branch,
        loc: task.loc,
        elapsed: started.elapsed(),
        tokens,
        exceeded,
        exceeded_message: token_budget.exceeded_message(),
        result,
    }
}

/// Build the single-phase prompt: the existing loop prompt for exactly one
/// loop, plus this phase's handoff contract and the upstream artifacts the
/// dependency gate guarantees exist.
fn build_phase_prompt(
    plan: &RunPlan,
    index: usize,
    loc: &epic::LoopLocation,
    branch: &str,
) -> String {
    let phase = &plan.phases[index];
    let phase_timeout = resolved_phase_timeout(plan);
    let run_timeout = resolved_run_timeout(plan);
    let mut prompt = build_combined_phase_prompt(
        &[(
            phase.execution_id.clone(),
            loc.clone(),
            phase.interview.clone(),
        )],
        plan.consent.draft_pr_authorized,
        ResolvedBudgets {
            phase_token_cap: resolved_phase_token_cap(plan),
            phase_wall_clock_secs: phase_timeout.as_secs(),
            run_wall_clock_secs: run_timeout.as_secs(),
        },
    );
    prompt.push_str(&format!(
        "\n\nThis phase runs in its own worktree on branch `{branch}`, cut from the repository's default branch. Do not merge or rebase onto another phase's branch."
    ));
    if let Some(topic) = phase.handoff_topic.as_deref() {
        // Only phases opted into a handoff carry an artifact contract; the
        // topic is fixed by the plan so a downstream consumer's reference
        // never changes.
        prompt.push_str(&format!(
            "\n\nThis phase must write its required contract artifact to `.loopdeck/handoff-artifacts/{topic}.md`. If the general instructions above name a different artifact topic, this configured topic wins. Use YAML frontmatter with `artifact: {topic}`, `author_role` set to the assigned role name or `planner`, `phase: {}`, `type: report`, `created` as today's date, and a one-line `summary`; then include `## Summary` and concise sections matching `docs/epics/role-based-orchestration/handoff-artifact-contract.md`. Do not choose or derive a different topic.",
            phase.execution_id
        ));
    }
    let upstream: Vec<(&str, &str)> = phase
        .depends_on
        .iter()
        .filter_map(|dependency| {
            plan.phases
                .iter()
                .find(|candidate| &candidate.execution_id == dependency)
                .and_then(|upstream| upstream.handoff_topic.as_deref())
                .map(|topic| (dependency.as_str(), topic))
        })
        .collect();
    if !upstream.is_empty() {
        prompt.push_str(
            "\n\nUpstream handoff artifacts (already published by completed phases; read them from this worktree):\n",
        );
        for (dependency, topic) in upstream {
            prompt.push_str(&format!(
                "- phase `{dependency}` → `.loopdeck/handoffs/{topic}.md`\n"
            ));
        }
        prompt.push_str(
            "Depend only on these artifacts — never on another phase's branch code. In your own artifact, cite every section and item you used in a `## Handoff citations` block per the contract.",
        );
    }
    prompt
}

/// Promote a phase into `execution.yaml`'s `current` pointer, but only when
/// no sibling owns it. `run-plan.yaml` is the authoritative per-phase record;
/// `current` is a UI-focus pointer and must never make two parallel turns
/// fight over one slot.
fn begin_phase_execution(
    root: &Path,
    execution_id: &str,
    loc: &epic::LoopLocation,
) -> Result<(), AppError> {
    let loaded = execution::load(root)?;
    if let Some(current) = loaded.state.current.as_ref() {
        if current.id != execution_id {
            return Ok(());
        }
        // Resume of the phase that already owns the pointer: nothing to do.
        return Ok(());
    }
    let promoted = loaded.state.promote_loop_into_current(
        execution_id,
        &loc.title,
        LoopOrigin {
            epic: loc.epic.clone(),
            prd: loc.prd.clone(),
            phase: loc.phase.clone(),
        },
        Utc::now(),
    )?;
    execution::save(root, &promoted, loaded.state.revision)?;
    Ok(())
}

/// Terminal bookkeeping for a non-green phase. Only touches `current` when
/// this phase owns it; a parallel sibling's focus pointer is left alone.
fn end_phase_execution(root: &Path, execution_id: &str, reason: String) -> Result<(), AppError> {
    let loaded = execution::load(root)?;
    let owns_current = loaded
        .state
        .current
        .as_ref()
        .map(|current| current.id == execution_id)
        .unwrap_or(false);
    if !owns_current {
        return Ok(());
    }
    let abandoned = loaded.state.abandon_current(reason, Utc::now(), false)?;
    execution::save(root, &abandoned, loaded.state.revision)?;
    Ok(())
}

/// Green-phase bookkeeping: relink or complete the current loop and attach
/// the delivery evidence, but only when this phase owns `current`.
#[allow(clippy::too_many_arguments)]
fn complete_phase_execution(
    root: &Path,
    execution_id: &str,
    final_id: &str,
    replacement: Option<&epic::LoopLocation>,
    branch: &str,
    worktree: &Path,
    url: &str,
    rubric: Option<delivery::RubricResult>,
) -> Result<(), AppError> {
    let loaded = execution::load(root)?;
    let owns_current = loaded
        .state
        .current
        .as_ref()
        .map(|current| current.id == execution_id)
        .unwrap_or(false);
    if !owns_current {
        return Ok(());
    }
    let revision = loaded.state.revision;
    let mut state = loaded.state;
    if let Some(current) = state.current.as_mut() {
        current.id = final_id.to_string();
        if let Some(location) = replacement {
            current.title = location.title.clone();
            current.origin = LoopOrigin {
                epic: location.epic.clone(),
                prd: location.prd.clone(),
                phase: location.phase.clone(),
            };
        }
        current.delivery = Some(delivery::DeliveryLinks {
            branch: Some(branch.to_string()),
            commit: git::head_commit(worktree),
            pr_url: Some(url.to_string()),
            pr_provider: url.contains("github.com").then(|| "github".to_string()),
            rubric,
        });
    }
    let completed = state.complete_current(Utc::now(), None, false)?;
    execution::save(root, &completed, revision)?;
    Ok(())
}

/// Park one phase with a reason, fail its not-yet-started dependents, and
/// persist. Independent in-flight phases keep running (approved gated-start
/// answer).
fn park_phase(
    root: &Path,
    plan: &mut RunPlan,
    index: usize,
    reason: String,
) -> Result<(), AppError> {
    let execution_id = plan.phases[index].execution_id.clone();
    plan.phases[index].status = RunPhaseStatus::Parked;
    plan.phases[index].park_payload = Some(reason.clone());
    plan.environment.worktree_kept = true;
    scheduler::fail_dependents(plan, &execution_id, &reason);
    runplan::save(root, plan)?;
    end_phase_execution(root, &execution_id, reason)?;
    Ok(())
}

/// Apply one finished phase turn to the plan. Runs **only** in the scheduler
/// loop, which serializes every durable write; phase tasks themselves never
/// touch the plan.
fn apply_phase_outcome(
    app: &AppHandle,
    state: &AppState,
    root: &Path,
    plan: &mut RunPlan,
    run_started: Instant,
    outcome: PhaseTurnOutcome,
) -> Result<(), AppError> {
    let index = outcome.index;
    let execution_id = outcome.execution_id.clone();
    plan.phases[index].wall_clock_secs = outcome.elapsed.as_secs();
    plan.phases[index].token_usage = outcome.tokens;
    plan.wall_clock_secs = run_started.elapsed().as_secs();

    if outcome.exceeded {
        // Per-phase budget kill: end only this phase, never the run.
        let reason = outcome.exceeded_message.clone();
        plan.phases[index].status = RunPhaseStatus::Killed;
        plan.phases[index].park_payload = Some(reason.clone());
        plan.environment.worktree_kept = true;
        scheduler::fail_dependents(plan, &execution_id, &reason);
        runplan::save(root, plan)?;
        let _ = terminate_session(state, &outcome.worktree);
        end_phase_execution(root, &execution_id, reason)?;
        notify_run_terminal(
            app,
            root,
            "Selasar phase killed",
            "A phase exceeded its token budget — independent phases continue",
        );
        return Ok(());
    }

    match outcome.result {
        Ok(response) => {
            if !response.session_id.is_empty() {
                plan.phases[index].session_id = Some(response.session_id.clone());
            }
            let verdict = extract_verdict(&response.result);
            if verdict == Some(RunVerdict::Pass) {
                let topic = plan.phases[index].handoff_topic.clone();
                if let Err(error) =
                    emit_phase_artifact(root, &outcome.worktree, &execution_id, topic.as_deref())
                {
                    return park_phase(
                        root,
                        plan,
                        index,
                        format!("handoff artifact emission failed: {error}"),
                    );
                }
            }
            if verdict == Some(RunVerdict::Pass) && plan.consent.draft_pr_authorized {
                // Retained before any delivery decision so a recoverable
                // record can carry the rubric evidence (retry-recovery).
                let rubric = delivery::extract_rubric_result(&response.result, chrono::Utc::now());
                // The PR link: the turn's own report first; then — the
                // crash-retry path — an already-open PR for this phase branch.
                let url = extract_draft_pr_url(&response.result)
                    .or_else(|| git::open_pr_for_branch(&outcome.worktree, &outcome.branch));
                let Some(url) = url else {
                    let reason =
                        "green verification passed, but no unattended draft PR URL was recorded";
                    // Recoverable delivery record (Phase 4, `retry-recovery`):
                    // how far delivery got in Git and why it stopped, so one
                    // idempotent retry command can resume from that stage. No
                    // automatic retry.
                    if let Err(error) = delivery_retry::save(
                        root,
                        &delivery_retry::DeliveryRetryRecord {
                            execution_ids: vec![execution_id.clone()],
                            branch: outcome.branch.clone(),
                            worktree: outcome.worktree.clone(),
                            stage: delivery_retry::detect_stage(&outcome.worktree),
                            reason: reason.to_string(),
                            rubric: rubric.clone(),
                            pr_title: outcome.loc.title.clone(),
                            recorded_at: chrono::Utc::now(),
                        },
                    ) {
                        warn!("could not persist the recoverable delivery record: {error}");
                    }
                    return park_phase(root, plan, index, reason.to_string());
                };

                // Delivery gates, evaluated fresh from this turn's own
                // verifier report — never a stale persisted verdict.
                // Idempotent finish is exempt: when the checklist item is
                // already checked and an open PR claims the branch, this is a
                // retry completing bookkeeping, not a new delivery.
                let already_delivered = epic::loop_checked(root, &execution_id) == Some(true);
                if !already_delivered {
                    let branch_matches = git::current_branch(&outcome.worktree)
                        .map(|live| live == outcome.branch)
                        .unwrap_or(false);
                    let checked = epic::loop_checked(root, &execution_id);
                    let blocks = delivery::evaluate_delivery_gates(
                        checked.map(|checked| !checked),
                        branch_matches,
                        checked.is_some(),
                        rubric.as_ref(),
                    );
                    if !blocks.is_empty() {
                        let reason = format!(
                            "delivery gate blocked: {}",
                            blocks
                                .iter()
                                .map(std::string::ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("; ")
                        );
                        return park_phase(root, plan, index, reason);
                    }
                }

                let replacement_id = if epic::find_loop_by_id(root, &execution_id).is_some() {
                    None
                } else {
                    epic::find_replacement_loop_id(root, &outcome.loc)
                };
                let final_id = replacement_id
                    .clone()
                    .unwrap_or_else(|| execution_id.clone());
                let replacement_loc = replacement_id
                    .as_deref()
                    .and_then(|id| epic::find_loop_by_id(root, id));
                let final_status = if epic::find_loop_by_id(root, &final_id).is_some() {
                    RunPhaseStatus::Completed
                } else {
                    RunPhaseStatus::Delivered
                };
                plan.phases[index].execution_id = final_id.clone();
                plan.phases[index].status = final_status;
                plan.phases[index].park_payload = Some(
                    if final_status == RunPhaseStatus::Completed {
                        format!("draft PR: {url}")
                    } else {
                        format!(
                        "draft PR: {url}\ndelivered — PRD link needs repair before this can appear in progress"
                    )
                    },
                );
                if replacement_id.is_some() {
                    // Keep downstream edges pointing at the surviving id.
                    for phase in &mut plan.phases {
                        for dependency in &mut phase.depends_on {
                            if dependency == &execution_id {
                                *dependency = final_id.clone();
                            }
                        }
                    }
                }
                // The PR URL is the delivery boundary: only now may the
                // checklist declare the loop done. Idempotent.
                epic::complete_prd_loop(
                    root,
                    &outcome.loc.epic,
                    &outcome.loc.prd,
                    &outcome.loc.title,
                )?;
                runplan::save(root, plan)?;
                complete_phase_execution(
                    root,
                    &execution_id,
                    &final_id,
                    replacement_loc.as_ref(),
                    &outcome.branch,
                    &outcome.worktree,
                    &url,
                    rubric,
                )?;
                // Clean handoff: this branch, PR, and worktree stay for
                // review; the next phase/run starts from the default branch.
                record_handoff(root, &outcome.branch, &outcome.worktree, &url);
                return Ok(());
            }

            let reason = match verdict {
                Some(RunVerdict::Pass) => "green verification passed, but queue-time consent for an unattended draft PR is required".to_string(),
                Some(RunVerdict::Warn) => "verify verdict: WARN".to_string(),
                Some(RunVerdict::Block) => "verify verdict: BLOCK".to_string(),
                _ => "no verify verdict found in the turn's final response".to_string(),
            };
            park_phase(root, plan, index, reason)
        }
        Err(AppError::TurnParked {
            detail,
            parked_questions_json,
        }) => {
            // Store the structured question payload when available, so the
            // morning report can reconstruct AskUserQuestionCard components.
            let payload = match parked_questions_json {
                Some(ref json) => format!("__QUESTIONS__{json}__END__ {detail}"),
                None => detail,
            };
            park_phase(root, plan, index, payload)
        }
        Err(error) => {
            let message = error.to_string();
            let killed_by_budget = message.contains("wall-clock budget exceeded")
                || message.contains("token budget exceeded");
            let status = if killed_by_budget {
                RunPhaseStatus::Killed
            } else {
                RunPhaseStatus::Failed
            };
            plan.phases[index].status = status;
            plan.phases[index].park_payload = Some(message.clone());
            plan.environment.worktree_kept = true;
            // A non-green phase fails its dependents; independent in-flight
            // phases continue (approved gated-start answer).
            scheduler::fail_dependents(plan, &execution_id, &message);
            runplan::save(root, plan)?;
            if message.contains("token budget exceeded") {
                let _ = terminate_session(state, &outcome.worktree);
            }
            end_phase_execution(root, &execution_id, message.clone())?;
            if killed_by_budget {
                notify_run_terminal(
                    app,
                    root,
                    "Selasar phase killed",
                    "A phase hit its budget — independent phases continue",
                );
            }
            Ok(())
        }
    }
}

/// Mark every unfinished phase terminal with one reason. Used by the run-level
/// cancel and total-wall-clock paths, which stop the whole run by design.
fn stop_unfinished_phases(plan: &mut RunPlan, reason: &str) {
    for phase in &mut plan.phases {
        if scheduler::is_active(phase.status) {
            phase.status = RunPhaseStatus::Killed;
            phase.park_payload = Some(reason.to_string());
        }
    }
}

/// The run executor. One turn per run-plan phase, with phases whose
/// `depends_on` edges are satisfied running concurrently in separate
/// worktrees up to `RunPlan::max_concurrency`.
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
    ignore_managed_runs_dir(root);
    // A cycle would leave every member Waiting forever; reject before any
    // worktree or provider process exists.
    scheduler::validate_dependency_graph(&plan.phases)?;
    // Legacy plans start all-Queued; compute the initial gate now. Then
    // release any phase whose upstream already completed in a prior attempt.
    scheduler::mark_waiting(&mut plan);
    scheduler::reconcile_gates(&mut plan);
    runplan::save(root, &plan)?;

    let mut tasks: tokio::task::JoinSet<PhaseTurnOutcome> = tokio::task::JoinSet::new();
    let mut running: HashMap<usize, PathBuf> = HashMap::new();

    loop {
        plan.wall_clock_secs = run_started.elapsed().as_secs();

        if cancel.load(Ordering::SeqCst) {
            for worktree in running.values() {
                let _ = terminate_session(state, worktree);
                let _ = fire_interrupt(state, worktree);
            }
            stop_unfinished_phases(&mut plan, "run cancelled");
            plan.environment.worktree_kept = true;
            runplan::save(root, &plan)?;
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            // Aborting a task after it inserted its session leaves the child
            // in the cache map; evict again so no orphan process survives.
            for worktree in running.values() {
                let _ = terminate_session(state, worktree);
            }
            notify_run_terminal(app, root, "Selasar run killed", "Run was cancelled");
            return Ok(());
        }

        if run_started.elapsed() >= resolved_run_timeout(&plan) {
            for worktree in running.values() {
                let _ = terminate_session(state, worktree);
            }
            stop_unfinished_phases(&mut plan, "total run wall-clock budget exceeded");
            plan.environment.worktree_kept = true;
            runplan::save(root, &plan)?;
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            for worktree in running.values() {
                let _ = terminate_session(state, worktree);
            }
            notify_run_terminal(
                app,
                root,
                "Selasar run killed",
                "Total run wall-clock budget exceeded",
            );
            return Ok(());
        }

        // Open the gate for any phase whose upstreams all landed green, and
        // fail dependents of upstreams that did not.
        let (released, failed) = scheduler::reconcile_gates(&mut plan);
        if released + failed > 0 {
            runplan::save(root, &plan)?;
        }

        // Fill spare concurrency slots with ready phases.
        let max = plan.max_concurrency.max(1) as usize;
        let slots = max.saturating_sub(running.len());
        if slots > 0 {
            let in_flight: HashSet<String> = running
                .keys()
                .map(|&index| plan.phases[index].execution_id.clone())
                .collect();
            for index in scheduler::select_ready(&plan, &in_flight, slots) {
                match prepare_phase(app, state, root, &mut plan, index, run_started) {
                    Ok(task) => {
                        let task_index = task.index;
                        let worktree = task.worktree.clone();
                        let app_handle = app.clone();
                        let run_root = root.to_path_buf();
                        let cancel_flag = Arc::clone(cancel);
                        plan.phases[index].status = RunPhaseStatus::Running;
                        plan.phases[index].park_payload = None;
                        runplan::save(root, &plan)?;
                        running.insert(task_index, worktree);
                        tasks.spawn(async move {
                            let state = app_handle.state::<AppState>();
                            run_phase_turn(&state, &run_root, &cancel_flag, task).await
                        });
                    }
                    Err(error) => {
                        // Preparation failure is local to this phase. A missing
                        // roster entry parks (recoverable via requeue); any
                        // other preparation error fails the phase. Either way
                        // its dependents fail and independent phases continue.
                        let (status, reason) = match error {
                            PhasePrepareError::Parked(reason) => (RunPhaseStatus::Parked, reason),
                            PhasePrepareError::Failed(reason) => (
                                RunPhaseStatus::Failed,
                                format!("phase preparation failed: {reason}"),
                            ),
                        };
                        let execution_id = plan.phases[index].execution_id.clone();
                        plan.phases[index].status = status;
                        plan.phases[index].park_payload = Some(reason.clone());
                        plan.environment.worktree_kept = true;
                        scheduler::fail_dependents(&mut plan, &execution_id, &reason);
                        runplan::save(root, &plan)?;
                    }
                }
            }
        }

        if running.is_empty() {
            if !scheduler::has_active_phases(&plan) {
                break;
            }
            // Active phases but nothing runnable and nothing in flight: the
            // gate can never open (for example every upstream was parked
            // before the gate ran). Fail them instead of spinning.
            let stalled: Vec<String> = plan
                .phases
                .iter()
                .filter(|phase| scheduler::is_active(phase.status))
                .map(|phase| phase.execution_id.clone())
                .collect();
            for phase in &mut plan.phases {
                if scheduler::is_active(phase.status) {
                    phase.status = RunPhaseStatus::Failed;
                    phase.park_payload = Some(format!(
                        "scheduler stalled: no runnable phase (dependency gate blocked: {})",
                        stalled.join(", ")
                    ));
                }
            }
            plan.environment.worktree_kept = true;
            runplan::save(root, &plan)?;
            break;
        }

        // Wait for one phase to finish, but wake periodically so cancel and
        // the total-run cap stay responsive while long turns run.
        tokio::select! {
            joined = tasks.join_next() => {
                match joined {
                    Some(Ok(outcome)) => {
                        running.remove(&outcome.index);
                        if let Err(error) =
                            apply_phase_outcome(app, state, root, &mut plan, run_started, outcome)
                        {
                            // Bookkeeping failure is fatal for the run, but
                            // sibling turns must not be left orphaned.
                            for worktree in running.values() {
                                let _ = terminate_session(state, worktree);
                            }
                            tasks.abort_all();
                            while tasks.join_next().await.is_some() {}
                            return Err(error);
                        }
                    }
                    Some(Err(join_error)) => {
                        // A panicked turn task is unrecoverable for the phases
                        // it owned; their statuses cannot be trusted.
                        let reason = format!("phase task failed unexpectedly: {join_error}");
                        for (index, _) in running.drain() {
                            plan.phases[index].status = RunPhaseStatus::Failed;
                            plan.phases[index].park_payload = Some(reason.clone());
                            let execution_id = plan.phases[index].execution_id.clone();
                            scheduler::fail_dependents(&mut plan, &execution_id, &reason);
                        }
                        plan.environment.worktree_kept = true;
                        runplan::save(root, &plan)?;
                    }
                    None => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }

    plan.wall_clock_secs = run_started.elapsed().as_secs();
    finalize_worktree(&mut plan);
    runplan::save(root, &plan)?;

    let all_completed = plan.phases.iter().all(|p| {
        matches!(
            p.status,
            RunPhaseStatus::Completed | RunPhaseStatus::Delivered
        )
    });
    let any_parked = plan
        .phases
        .iter()
        .any(|p| matches!(p.status, RunPhaseStatus::Parked | RunPhaseStatus::Waiting));

    if all_completed {
        notify_run_terminal(
            app,
            &plan.project,
            "Selasar run completed",
            "All phases passed — morning report ready for review",
        );
    } else if any_parked {
        notify_run_terminal(
            app,
            &plan.project,
            "Selasar run parked",
            "Phases need attention — open the morning report to review parked items",
        );
    }

    Ok(())
}

/// Why a phase could not be prepared. A missing roster entry is recoverable
/// (park, user fixes the roster and retries); every other preparation error
/// is a hard failure for that phase only.
enum PhasePrepareError {
    Failed(String),
    Parked(String),
}

/// Prepare one ready phase: create or recover its worktree, resolve staffing,
/// seed upstream artifacts, build the single-phase prompt, and promote it in
/// `execution.yaml` when the focus pointer is free. Returns the task to spawn.
/// Persists the phase's worktree identity before the turn starts, so a crash
/// leaves truthful on-disk state.
fn prepare_phase(
    app: &AppHandle,
    state: &AppState,
    root: &Path,
    plan: &mut RunPlan,
    index: usize,
    run_started: Instant,
) -> Result<PhaseTask, PhasePrepareError> {
    let execution_id = plan.phases[index].execution_id.clone();
    let Some(loc) = epic::find_loop_by_id(root, &execution_id) else {
        return Err(PhasePrepareError::Failed(format!(
            "queued phase \"{execution_id}\" no longer exists in docs/epics/"
        )));
    };

    let agent_config = match plan.phases[index].assigned_agent_id.as_deref() {
        Some(agent_id) => resolve_agent_config_by_id(state, agent_id).map_err(|error| {
            PhasePrepareError::Parked(format!(
                "assigned agent \"{agent_id}\" is not on the roster: {error}"
            ))
        })?,
        None => resolve_agent_config(state)
            .map_err(|error| PhasePrepareError::Failed(error.to_string()))?,
    };

    let (worktree, branch, _created) = ensure_phase_worktree(root, plan, index)
        .map_err(|error| PhasePrepareError::Failed(error.to_string()))?;
    plan.phases[index].worktree_path = Some(worktree.clone());
    plan.phases[index].worktree_branch = Some(branch.clone());
    if let Err(error) = handoff::seed_worktree_store(root, &worktree) {
        warn!(
            "could not seed upstream handoff artifacts into {}: {error}",
            worktree.display()
        );
    }
    let prompt = build_phase_prompt(plan, index, &loc, &branch);
    let resume_session_id = plan.phases[index].resume_session_id.clone();
    plan.phases[index].resume_session_id = None;
    begin_phase_execution(root, &execution_id, &loc)
        .map_err(|error| PhasePrepareError::Failed(error.to_string()))?;

    let event_app = app.clone();
    let event_project_path = root.to_string_lossy().into_owned();
    let channel_execution_id = execution_id.clone();
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
                execution_id: channel_execution_id.clone(),
                event,
            },
        ) {
            warn!("failed to emit unattended run activity: {error}");
        }
        Ok(())
    });
    let run_events = Some(RunEventChannel::new(
        plan.id.clone(),
        normalized_run_channel(
            app.clone(),
            root.to_string_lossy().into_owned(),
            execution_id.clone(),
        ),
    ));

    Ok(PhaseTask {
        index,
        execution_id,
        worktree,
        branch,
        title: loc.title.clone(),
        loc,
        prompt,
        agent_config,
        channel,
        run_events,
        token_cap: resolved_phase_token_cap(plan),
        phase_timeout: resolved_phase_timeout(plan),
        run_timeout: resolved_run_timeout(plan).saturating_sub(run_started.elapsed()),
        resume_session_id,
    })
}

fn finalize_worktree(plan: &mut RunPlan) {
    plan.environment.worktree_kept = true;
}

#[cfg(test)]
mod unattended_tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn unavailable_codex_input_is_preserved_as_a_parked_question() {
        let questions = unavailable_codex_questions(&[crate::runplan::PinnedAnswer {
            question: "Which existing roster entries should the unattended demo use?".into(),
            answer: "(unanswered — native input request unavailable in this mode)".into(),
        }]);

        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].header, "Pre-flight question");
        assert!(questions[0].options.is_empty());
        let payload = preflight_question_payload(&questions).expect("payload");
        assert!(payload.starts_with("__QUESTIONS__["));
        assert!(payload.contains("__END__"));
    }

    fn plan() -> RunPlan {
        run_executor::build_run_plan(
            "run-budget-test".into(),
            PathBuf::from("/repo"),
            Utc::now(),
            &["phase-1".into()],
            StallPolicy::ContinueIndependent,
            false,
            &[],
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
            &[],
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
    fn delivered_phase_is_never_retryable() {
        let mut plan = parked_chain();
        plan.phases[0].status = RunPhaseStatus::Delivered;
        plan.phases[0].park_payload = Some("draft PR: https://github.com/acme/repo/pull/42".into());

        let error = requeue_terminal_phase(&mut plan, "phase-a")
            .expect_err("delivered work must be relinked, never rerun");
        assert!(error.to_string().contains("not retryable"));
        assert_eq!(plan.phases[0].status, RunPhaseStatus::Delivered);
        assert_eq!(requeue_all_terminal_phases(&mut plan), 0);
    }

    #[test]
    fn requeue_all_terminal_phases_requeues_every_retryable_status() {
        let mut plan = parked_chain();
        plan.phases[0].status = RunPhaseStatus::Failed;
        plan.phases[0].park_payload = Some("harness spawn failed".into());
        plan.phases[1].status = RunPhaseStatus::Killed;
        plan.phases[1].park_payload = Some("total run wall-clock budget exceeded".into());
        plan.phases[2].status = RunPhaseStatus::Parked;
        plan.phases[2].park_payload = Some("verify verdict: BLOCK".into());

        let requeued = requeue_all_terminal_phases(&mut plan);

        assert_eq!(requeued, 3);
        assert!(plan
            .phases
            .iter()
            .all(|phase| phase.status == RunPhaseStatus::Queued));
        assert!(plan.phases.iter().all(|phase| phase.park_payload.is_none()));
    }

    #[test]
    fn requeue_all_terminal_phases_leaves_active_and_completed_phases_alone() {
        let mut plan = parked_chain();
        plan.phases[0].status = RunPhaseStatus::Completed;
        plan.phases[1].status = RunPhaseStatus::Running;
        plan.phases[2].status = RunPhaseStatus::Interrupted;
        plan.phases[2].park_payload = Some("turn interrupted".into());

        let requeued = requeue_all_terminal_phases(&mut plan);

        assert_eq!(requeued, 1);
        assert_eq!(plan.phases[0].status, RunPhaseStatus::Completed);
        assert_eq!(plan.phases[1].status, RunPhaseStatus::Running);
        assert_eq!(plan.phases[2].status, RunPhaseStatus::Queued);
        assert!(plan.phases[2].park_payload.is_none());
    }

    #[test]
    fn requeue_all_terminal_phases_counts_zero_when_nothing_is_retryable() {
        let mut plan = parked_chain();
        for phase in &mut plan.phases {
            phase.status = RunPhaseStatus::Completed;
        }

        assert_eq!(requeue_all_terminal_phases(&mut plan), 0);
    }

    // ── Handoff topics (`prd-agent-handoff` Phase 1) ─────────────────────

    fn topic(execution_id: &str, topic: &str) -> runplan::PhaseHandoffTopic {
        runplan::PhaseHandoffTopic {
            execution_id: execution_id.into(),
            topic: topic.into(),
        }
    }

    #[test]
    fn apply_handoff_topics_configures_only_the_listed_phases() {
        let mut plan = parked_chain();
        apply_handoff_topics(
            &mut plan,
            &["phase-a".into(), "phase-b".into(), "phase-c".into()],
            vec![topic("phase-a", "phase-a-notes")],
        )
        .expect("valid topic applies");

        assert_eq!(
            plan.phases[0].handoff_topic.as_deref(),
            Some("phase-a-notes")
        );
        assert!(plan.phases[1].handoff_topic.is_none());
        assert!(plan.phases[2].handoff_topic.is_none());
    }

    #[test]
    fn apply_handoff_topics_rejects_a_duplicate_topic_across_phases() {
        let mut plan = parked_chain();
        let error = apply_handoff_topics(
            &mut plan,
            &["phase-a".into(), "phase-b".into()],
            vec![topic("phase-a", "shared"), topic("phase-b", "shared")],
        )
        .expect_err("one topic maps to one file");

        assert!(error.to_string().contains("unique kebab-case slugs"));
    }

    #[test]
    fn apply_handoff_topics_rejects_a_non_kebab_slug_or_unknown_phase() {
        let mut plan = parked_chain();
        let ids = ["phase-a".into(), "phase-b".into()];
        assert!(
            apply_handoff_topics(&mut plan, &ids, vec![topic("phase-a", "Phase A Notes")],)
                .is_err()
        );
        assert!(
            apply_handoff_topics(&mut plan, &ids, vec![topic("not-in-plan", "topic")],).is_err()
        );
        // A rejected topic leaves the plan untouched.
        assert!(plan
            .phases
            .iter()
            .all(|phase| phase.handoff_topic.is_none()));
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
            multi_agent_active_runs: std::sync::Mutex::new(std::collections::HashSet::new()),
            multi_agent_manifest_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
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
        // `run_worktree_path` derives the destination from the branch name,
        // whose unique run-id suffix keeps lifecycle tests isolated even when
        // they share a parent directory.
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
            assigned_agent: None,
            park_payload: None,
            token_usage: 0,
            wall_clock_secs: 0,
            ..Default::default()
        }
    }

    fn failed_phase(id: &str) -> runplan::RunPhase {
        runplan::RunPhase {
            status: RunPhaseStatus::Failed,
            ..completed_phase(id)
        }
    }

    /// Create: `ensure_phase_worktree` on a fresh plan creates a real linked
    /// worktree, cuts it from the default branch, and returns the identity the
    /// caller persists on the phase.
    #[test]
    fn ensure_phase_worktree_creates_a_phase_scoped_worktree() {
        let dir = temp_git_repo("create");
        let plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);

        let (worktree, branch, created) =
            ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");

        assert!(created);
        assert!(branch.starts_with("run/phase-1-p1-"), "{branch}");
        assert!(git::worktree_list(&dir).unwrap().contains(&worktree));

        git::worktree_remove(&dir, &worktree).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Clean handoff (`prd-verified-delivery-reconciliation` Phase 4): a new
    /// phase branch is cut from the default branch, not from whatever the main
    /// worktree has checked out. A fix committed to a stray branch must not
    /// ride into the next phase's base.
    #[test]
    fn ensure_phase_worktree_bases_new_branch_on_default_branch_not_head() {
        let dir = temp_git_repo("base");
        let run_git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(["-C"])
                .arg(&dir)
                .args(args)
                .output()
                .unwrap()
        };
        run_git(&["branch", "-m", "main"]);
        let main_tip = String::from_utf8(run_git(&["rev-parse", "main"]).stdout).unwrap();
        // Stray branch with a commit the main worktree now sits on.
        run_git(&["checkout", "-q", "-b", "stray-fix"]);
        std::fs::write(dir.join("stray.txt"), "not for the next run\n").unwrap();
        run_git(&["add", "."]);
        run_git(&["commit", "-m", "stray fix"]);
        let stray_tip = String::from_utf8(run_git(&["rev-parse", "HEAD"]).stdout).unwrap();

        let plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        let (worktree, _, _) = ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");

        let tip = String::from_utf8(
            std::process::Command::new("git")
                .args(["-C"])
                .arg(&worktree)
                .arg("rev-parse")
                .arg("HEAD")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert_eq!(
            tip.trim(),
            main_tip.trim(),
            "phase branch must start at main"
        );
        assert_ne!(tip.trim(), stray_tip.trim());
        assert!(!worktree.join("stray.txt").exists());

        run_git(&["worktree", "remove", "--force", worktree.to_str().unwrap()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Clean handoff record persists at delivery and reloads with the
    /// delivered branch, PR URL, retained worktree, and next base.
    #[test]
    fn record_handoff_persists_a_loadable_handoff_record() {
        let dir = temp_git_repo("handoff");
        let plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        let (worktree, branch, _) = ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");

        record_handoff(&dir, &branch, &worktree, "https://github.com/o/r/pull/42");

        let record = handoff::load(&dir)
            .unwrap()
            .expect("handoff record written");
        assert_eq!(record.delivered_branch, branch);
        assert_eq!(record.pr_url, "https://github.com/o/r/pull/42");
        assert_eq!(record.worktree, worktree);
        assert_eq!(record.next_base, git::default_branch(&dir).unwrap());

        std::process::Command::new("git")
            .args(["-C"])
            .arg(&dir)
            .args(["worktree", "remove", "--force"])
            .arg(&worktree)
            .output()
            .unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Resume: calling `ensure_phase_worktree` again for a phase whose
    /// recorded worktree still exists is idempotent — no second worktree or
    /// branch.
    #[test]
    fn ensure_phase_worktree_resumes_without_duplicating() {
        let dir = temp_git_repo("resume");
        let mut plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        let (first, first_branch, _) =
            ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");
        plan.phases[0].worktree_path = Some(first.clone());
        plan.phases[0].worktree_branch = Some(first_branch.clone());

        let (second, second_branch, created) =
            ensure_phase_worktree(&dir, &plan, 0).expect("resume worktree");

        assert_eq!(first, second);
        assert_eq!(first_branch, second_branch);
        assert!(!created);
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

    /// Resume-from-branch (`prd-verified-delivery-reconciliation` Phase 2):
    /// a recorded worktree removed from Git's inventory while its branch
    /// survived is recreated at the same path from that branch — no second
    /// branch, no hard stop.
    #[test]
    fn ensure_phase_worktree_recreates_from_surviving_branch() {
        let dir = temp_git_repo("recreate");
        let mut plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        let (first, branch, _) = ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");
        plan.phases[0].worktree_path = Some(first.clone());
        plan.phases[0].worktree_branch = Some(branch.clone());

        git::worktree_remove(&dir, &first).expect("remove worktree, keep branch");
        assert!(!first.exists());
        assert!(git::branch_exists(&dir, &branch));

        let (second, second_branch, created) =
            ensure_phase_worktree(&dir, &plan, 0).expect("recreate from branch");

        assert_eq!(first, second, "recreated at the same recorded path");
        assert_eq!(branch, second_branch);
        assert!(created);
        assert!(second.exists());

        git::worktree_remove(&dir, &second).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every phase worktree lives under `.loopdeck/runs/`, and the ignore
    /// rule keeping managed worktrees out of the main checkout's untracked
    /// view is appended exactly once.
    #[test]
    fn ensure_phase_worktree_ignores_managed_runs_dir_once() {
        let dir = temp_git_repo("ignore");
        let plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);

        let (worktree, _, _) = ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");

        // Git canonicalizes the worktree path (`/var` → `/private/var` on
        // macOS); compare against the canonical repo dir.
        let canonical_dir = std::fs::canonicalize(&dir).unwrap();
        assert!(worktree.starts_with(canonical_dir.join(".loopdeck").join("runs")));
        let gitignore = std::fs::read_to_string(dir.join(".gitignore")).unwrap();
        assert_eq!(
            gitignore
                .lines()
                .filter(|l| l.trim() == ".loopdeck/runs/")
                .count(),
            1,
            "ignore rule appended exactly once"
        );

        git::worktree_remove(&dir, &worktree).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Parallel scheduling: two phases get two disjoint worktrees on two
    /// branches cut from the same default tip. This is the cleanup-race
    /// guard: removing one phase's worktree must never touch its sibling.
    #[test]
    fn phase_worktrees_are_disjoint_and_cleanup_is_isolated() {
        let dir = temp_git_repo("parallel");
        let plan = plan_with_phases(
            &dir,
            vec![completed_phase("phase-a"), completed_phase("phase-b")],
        );

        let (first, first_branch, _) = ensure_phase_worktree(&dir, &plan, 0).expect("a");
        let (second, second_branch, _) = ensure_phase_worktree(&dir, &plan, 1).expect("b");

        assert_ne!(first, second);
        assert_ne!(first_branch, second_branch);
        // Git canonicalizes the worktree path (`/var` → `/private/var` on
        // macOS), so compare against the canonical repo root.
        let canonical_dir = std::fs::canonicalize(&dir).unwrap();
        assert!(first.starts_with(canonical_dir.join(".loopdeck")));
        assert!(second.starts_with(canonical_dir.join(".loopdeck")));

        git::worktree_remove(&dir, &first).expect("remove a");
        assert!(!first.exists());
        assert!(second.exists(), "sibling worktree must survive cleanup");

        git::worktree_remove(&dir, &second).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Retain-on-delivery (`clean-handoff`, superseding the old prune-on-
    /// success): every phase delivered → each phase worktree stays on disk
    /// under `.loopdeck/runs/`, flagged kept, with its recorded path/branch
    /// intact so the branch + PR stay reviewable.
    #[test]
    fn finalize_worktree_retains_every_delivered_phase_worktree() {
        let dir = temp_git_repo("retain");
        let plan = plan_with_phases(&dir, vec![completed_phase("phase-1")]);
        let (worktree, branch, _) = ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");

        let mut plan = plan;
        finalize_worktree(&mut plan);

        assert!(plan.environment.worktree_kept);
        assert!(
            worktree.exists(),
            "the delivered worktree must remain on disk"
        );
        assert!(git::branch_exists(&dir, &branch));

        git::worktree_remove(&dir, &worktree).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Keep-on-failure: any phase not delivered → the same retention, so a
    /// failed run stays inspectable.
    #[test]
    fn finalize_worktree_keeps_on_any_non_completed_phase() {
        let dir = temp_git_repo("keep-failed");
        let plan = plan_with_phases(
            &dir,
            vec![completed_phase("phase-1"), failed_phase("phase-2")],
        );
        let (worktree, _, _) = ensure_phase_worktree(&dir, &plan, 0).expect("create worktree");

        let mut plan = plan;
        finalize_worktree(&mut plan);

        assert!(plan.environment.worktree_kept);
        assert!(worktree.exists(), "a kept worktree must remain on disk");

        git::worktree_remove(&dir, &worktree).ok();
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
