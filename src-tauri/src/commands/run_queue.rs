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

use super::agent::start_fresh_and_record_streaming;
use super::state::{fire_interrupt, resolve_root, AppState};
use crate::agents::ClaudeEvent;
use crate::epic;
use crate::error::AppError;
use crate::execution::{self, LoopOrigin};
use crate::run_executor::{
    self, build_interview_prompt, build_phase_prompt, extract_interview_answers, extract_verdict,
    RunHandle, RunVerdict,
};
use crate::runplan::{
    self, InterviewStatus, RunBudgets, RunConsent, RunPhase, RunPhaseStatus, RunPlan, StallPolicy,
};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tracing::{info, warn};

/// Build a fresh `.loopdeck/run-plan.yaml` from a picker selection (PRD Phase
/// 5: "phase multi-select + Queue overnight run action").
///
/// `execution_ids` must be non-empty, duplicate-free, and each resolve to a
/// real loop under `docs/epics/` via [`epic::find_loop_by_id`] — the same
/// stable-ID join the executor itself relies on, so a typo or a stale ID from
/// a since-edited PRD is caught at queue-time, not hours into an unattended
/// run. `depends_on` defaults to the authored (selection) order, one phase
/// depending on its immediate predecessor — the PRD's Open Questions default
/// ("linear chain, no editor") for v1.
///
/// Every field starts fresh: `consent`/`budgets` at their defaults (no
/// unattended-ship authorization yet — that's a separate, explicit step) and
/// every phase `Queued`/interview `Pending`. Refuses to overwrite a plan
/// while a run is actively in progress for this project; otherwise a new
/// selection freely replaces whatever plan (finished or never-started) was on
/// disk, since a run plan has exactly one writer and no history worth
/// preserving once superseded.
#[tauri::command]
pub async fn create_run_plan(
    path: String,
    execution_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;

    if execution_ids.is_empty() {
        return Err(AppError::RunPlan(
            "select at least one phase to queue".into(),
        ));
    }

    {
        let handles = state.run_handles.lock().map_err(|_| AppError::LockError)?;
        if handles.contains_key(&root) {
            return Err(AppError::RunPlan(
                "a run is already in progress for this project".into(),
            ));
        }
    }

    let mut seen = HashSet::new();
    for id in &execution_ids {
        if !seen.insert(id.as_str()) {
            return Err(AppError::RunPlan(format!(
                "phase \"{id}\" was selected more than once"
            )));
        }
        if epic::find_loop_by_id(&root, id).is_none() {
            return Err(AppError::RunPlan(format!(
                "phase \"{id}\" was not found in docs/epics/"
            )));
        }
    }

    let phases = execution_ids
        .iter()
        .enumerate()
        .map(|(i, id)| RunPhase {
            execution_id: id.clone(),
            status: RunPhaseStatus::Queued,
            interview: Vec::new(),
            interview_status: InterviewStatus::Pending,
            depends_on: if i == 0 {
                Vec::new()
            } else {
                vec![execution_ids[i - 1].clone()]
            },
            park_payload: None,
        })
        .collect::<Vec<_>>();

    let plan = RunPlan {
        id: uuid::Uuid::new_v4().to_string(),
        project: root.clone(),
        created: chrono::Utc::now(),
        consent: RunConsent::default(),
        budgets: RunBudgets::default(),
        stall_policy: StallPolicy::default(),
        phases,
    };

    runplan::save(&root, &plan)?;
    info!(
        "run plan created for {} with {} phase(s)",
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

    // A `Running` phase left over from a prior crash isn't actually running
    // on this fresh process — reconcile before deciding whether a run is
    // already active, so a stale status can't block queuing forever.
    run_executor::reconcile_running_phases(&root)?;

    let plan = runplan::load(&root)?
        .ok_or_else(|| AppError::RunPlan("no run plan is queued for this project".into()))?;

    {
        let handles = state.run_handles.lock().map_err(|_| AppError::LockError)?;
        if handles.contains_key(&root) {
            return Err(AppError::RunPlan(
                "a run is already in progress for this project".into(),
            ));
        }
    }

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
        if let Err(e) = execute_run(&state, &run_root, &cancel).await {
            warn!(
                "run queue executor for {} ended with error: {e}",
                run_root.display()
            );
        }
        if let Ok(mut handles) = state.run_handles.lock() {
            handles.remove(&run_root);
        };
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

    fire_interrupt(&state, &root)?;
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
) -> Result<Option<RunPlan>, AppError> {
    let root = resolve_root(&state, &path)?;
    let has_handle = state
        .run_handles
        .lock()
        .map_err(|_| AppError::LockError)?
        .contains_key(&root);
    if !has_handle {
        run_executor::reconcile_running_phases(&root)?;
    }
    runplan::load(&root)
}

/// Run one queued phase's pre-flight interview turn (Phase 3) — a bounded
/// session driven through the *streaming* pipeline
/// (`start_fresh_and_record_streaming`, no-op sink `Channel`, same trick
/// Phase 4's executor uses). This is load-bearing, not cosmetic: per
/// `claude_session.rs::answer_ask_user_question`'s own doc comment, an
/// `AskUserQuestion` on the *non*-streaming path (`channel: None`) has no UI
/// surface to answer from and is auto-denied immediately instead of parking —
/// exactly the tool call `build_interview_prompt` tells the agent to make.
/// A channel merely being present (its callback can be a no-op) is what lets
/// `answer_control_request` park on the shared `question_slot` instead of
/// taking that deny branch; the pending card then surfaces through the
/// existing tab-agnostic `StuckQuestionCallout` (`ProjectDetail.tsx`), which
/// reads the same cross-project `AskUserQuestion` store every other pending
/// question does — no bespoke card needed in the run-queue UI (Phase 5).
/// Awaits the whole turn, including any parked question, so it only returns
/// once the user has answered (or the agent decided nothing was ambiguous) —
/// that's what "while the user is present" means here: the caller is expected
/// to be an active UI session, not a background poll.
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
    let response = start_fresh_and_record_streaming(&state, &root, &prompt, &channel).await?;
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
    state: &AppState,
    root: &Path,
    cancel: &Arc<AtomicBool>,
) -> Result<(), AppError> {
    let Some(mut plan) = runplan::load(root)? else {
        return Ok(());
    };

    for idx in 0..plan.phases.len() {
        if plan.phases[idx].status != RunPhaseStatus::Queued {
            continue;
        }
        if cancel.load(Ordering::SeqCst) {
            plan.phases[idx].status = RunPhaseStatus::Killed;
            runplan::save(root, &plan)?;
            return Ok(());
        }

        let execution_id = plan.phases[idx].execution_id.clone();
        let interview = plan.phases[idx].interview.clone();

        let loc = match epic::find_loop_by_id(root, &execution_id) {
            Some(loc) => loc,
            None => {
                plan.phases[idx].status = RunPhaseStatus::Failed;
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

        let prompt = build_phase_prompt(
            &execution_id,
            &loc,
            &interview,
            plan.consent.draft_pr_authorized,
        );
        // No-op sink: this run has no live UI channel of its own, but the
        // streaming pipeline is what makes AskUserQuestion/permission/plan
        // cards park instead of auto-deny — see this function's doc comment.
        let channel: Channel<ClaudeEvent> = Channel::new(|_| Ok(()));
        let outcome = start_fresh_and_record_streaming(state, root, &prompt, &channel).await;

        match outcome {
            Ok(response) => {
                let verdict = extract_verdict(&response.result);
                if verdict == Some(RunVerdict::Pass) {
                    plan.phases[idx].status = RunPhaseStatus::Completed;
                    runplan::save(root, &plan)?;

                    let loaded = execution::load(root)?;
                    let completed =
                        loaded
                            .state
                            .complete_current(chrono::Utc::now(), None, false)?;
                    execution::save(root, &completed, loaded.state.revision)?;
                } else {
                    let reason = match verdict {
                        Some(RunVerdict::Warn) => "verify verdict: WARN".to_string(),
                        Some(RunVerdict::Block) => "verify verdict: BLOCK".to_string(),
                        _ => "no verify verdict found in the turn's final response".to_string(),
                    };
                    plan.phases[idx].status = RunPhaseStatus::Failed;
                    plan.phases[idx].park_payload = Some(reason.clone());
                    runplan::save(root, &plan)?;

                    let loaded = execution::load(root)?;
                    let abandoned =
                        loaded
                            .state
                            .abandon_current(reason, chrono::Utc::now(), false)?;
                    execution::save(root, &abandoned, loaded.state.revision)?;
                    return Ok(());
                }
            }
            Err(AppError::TurnParked(detail)) => {
                plan.phases[idx].status = RunPhaseStatus::Parked;
                plan.phases[idx].park_payload = Some(detail.clone());

                for blocked_id in
                    run_executor::phases_blocked_by_park(&plan, &execution_id, plan.stall_policy)
                {
                    if let Some(p) = plan
                        .phases
                        .iter_mut()
                        .find(|p| p.execution_id == blocked_id)
                    {
                        p.status = RunPhaseStatus::Parked;
                        p.park_payload = Some(format!(
                            "blocked: depends on parked phase \"{execution_id}\""
                        ));
                    }
                }
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
                plan.phases[idx].status = RunPhaseStatus::Failed;
                plan.phases[idx].park_payload = Some(e.to_string());
                runplan::save(root, &plan)?;

                let loaded = execution::load(root)?;
                let abandoned =
                    loaded
                        .state
                        .abandon_current(e.to_string(), chrono::Utc::now(), false)?;
                execution::save(root, &abandoned, loaded.state.revision)?;
                return Err(e);
            }
        }
    }

    Ok(())
}
