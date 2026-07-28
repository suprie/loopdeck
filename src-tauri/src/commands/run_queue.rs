//! Run-queue executor commands (`prd-run-queue` Phase 2, milestone 0.4.0).
//!
//! Turns a persisted [`RunPlan`] into a sequence of orchestrated
//! `claude_session` turns, one per queued phase, advancing only on a green
//! (`PASS`) verify verdict (`run_executor::parse_verdict`). There is no
//! separate background-task registry: `queue_run`'s own async task *is* the
//! executor loop, and its future simply doesn't resolve until the run stops
//! (done, blocked on a non-`PASS` verdict, or cancelled). `get_run_status`
//! reads the persisted plan for live progress from any caller, including
//! while `queue_run` is still running; `cancel_run` flips a per-project flag
//! the loop checks between phases.
//!
//! **Known limitation:** each phase's turn runs through the existing
//! non-streaming `start_fresh_and_record` pipeline, which — like
//! `agent_start_loop`/`agent_send_message` — doesn't wire up the interrupt
//! slot (see `ClaudeSession::send_message`'s doc comment in
//! `claude_session.rs`). `cancel_run` therefore takes effect *between*
//! phases, not mid-turn; a hard mid-turn interrupt would need the streaming
//! pipeline + a headless event channel, deferred along with stall detection
//! to Phase 4. Mid-run stalls (a phase parking on a permission/question card)
//! aren't detected either yet — under today's non-streaming pipeline a
//! question is auto-denied and a permission parks up to `TURN_DEADLINE` (30
//! min) before the turn errors out; Phase 4 owns replacing that with real
//! park detection and the `StallPolicy` skip-ahead behavior.

use super::agent::start_fresh_and_record;
use super::execution::{complete_with_commit, promote_by_id};
use super::state::{resolve_root, AppState};
use crate::error::AppError;
use crate::run_executor::{self, Verdict};
use crate::runplan::{self, RunPhaseStatus, RunPlan};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;
use tracing::{debug, info, warn};

/// Persist `plan` and run its queued phases in order until the queue is
/// empty, a phase fails to reach a `PASS` verdict, or `cancel_run` is called.
/// Rejects immediately if a run is already in progress for this project.
///
/// Returns the final plan (also readable mid-run via `get_run_status`).
#[tauri::command]
pub async fn queue_run(
    path: String,
    plan: RunPlan,
    state: State<'_, AppState>,
) -> Result<RunPlan, AppError> {
    let root = resolve_root(&state, &path)?;
    let cancel = register_run(&state, &root)?;

    let mut plan = plan;
    runplan::save(&root, &plan)?;
    info!(
        "queue_run: starting {} phase(s) for {path}",
        plan.phases.len()
    );

    let outcome = execute(&state, &root, &path, &mut plan, &cancel).await;
    deregister_run(&state, &root);
    outcome?;
    Ok(plan)
}

/// Flag the in-progress run for `path` to stop before its next phase.
/// No-op (not an error) if no run is in flight — see the module doc for why
/// this doesn't interrupt a phase already mid-turn.
#[tauri::command]
pub async fn cancel_run(path: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let root = resolve_root(&state, &path)?;
    let guard = state.run_cancel.lock().map_err(|_| AppError::LockError)?;
    if let Some(flag) = guard.get(&root) {
        flag.store(true, Ordering::SeqCst);
        info!("cancel_run: flagged in-progress run for {path}");
    } else {
        debug!("cancel_run: no in-progress run for {path} (no-op)");
    }
    Ok(())
}

/// Read the persisted run plan for a project, if any. `None` means nothing
/// has ever been queued.
#[tauri::command]
pub async fn get_run_status(
    path: String,
    state: State<'_, AppState>,
) -> Result<Option<RunPlan>, AppError> {
    let root = resolve_root(&state, &path)?;
    runplan::load(&root)
}

/// Claim the run slot for `root`, erroring if one is already claimed.
fn register_run(state: &AppState, root: &Path) -> Result<Arc<AtomicBool>, AppError> {
    let mut guard = state.run_cancel.lock().map_err(|_| AppError::LockError)?;
    if guard.contains_key(root) {
        return Err(AppError::RunPlan(
            "a run is already in progress for this project".into(),
        ));
    }
    let flag = Arc::new(AtomicBool::new(false));
    guard.insert(root.to_path_buf(), Arc::clone(&flag));
    Ok(flag)
}

/// Release the run slot for `root`. Best-effort: a poisoned lock here would
/// mean the process is already in a bad state; nothing left to do but leave
/// the stale entry (a future run would surface a clear "already in progress"
/// rather than silently double-executing).
fn deregister_run(state: &AppState, root: &Path) {
    if let Ok(mut guard) = state.run_cancel.lock() {
        guard.remove(root);
    }
}

/// The executor loop: advance one phase at a time, stopping (not erroring)
/// the queue on cancellation, an empty/blocked queue, a session failure, or a
/// non-`PASS` verdict. `plan` is mutated and persisted after every phase
/// transition so `get_run_status` always reflects live progress.
async fn execute(
    state: &AppState,
    root: &Path,
    path: &str,
    plan: &mut RunPlan,
    cancel: &AtomicBool,
) -> Result<(), AppError> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            info!("queue_run: cancelled for {path}");
            return Ok(());
        }
        let Some(idx) = run_executor::next_eligible_phase(plan) else {
            info!("queue_run: no eligible phase left for {path}, stopping");
            return Ok(());
        };
        let execution_id = plan.phases[idx].execution_id.clone();

        // Promote first: resolves the phase's title from docs/epics/ (needed
        // for the prompt) and fails fast if another loop is already `current`
        // in execution.yaml — before this phase is marked `Running` and a
        // session is spawned for it.
        let promoted = promote_by_id(root, &execution_id)?;
        let title = promoted
            .current
            .as_ref()
            .map(|c| c.title.clone())
            .unwrap_or_else(|| execution_id.clone());

        plan.phases[idx].status = RunPhaseStatus::Running;
        runplan::save(root, plan)?;
        info!("queue_run: phase {execution_id} running for {path}");

        let prompt = run_executor::build_phase_prompt(&plan.phases[idx], &title);
        let response = start_fresh_and_record(state, root, &prompt).await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                warn!("queue_run: phase {execution_id} errored for {path}: {e}");
                plan.phases[idx].status = RunPhaseStatus::Failed;
                plan.phases[idx].park_payload = Some(e.to_string());
                runplan::save(root, plan)?;
                return Ok(());
            }
        };

        match run_executor::parse_verdict(&response.result) {
            Some(Verdict::Pass) => {
                complete_with_commit(root, None, false)?;
                plan.phases[idx].status = RunPhaseStatus::Completed;
                runplan::save(root, plan)?;
                info!("queue_run: phase {execution_id} completed (PASS) for {path}");
            }
            other => {
                let reason = match other {
                    Some(Verdict::Warn) => "verify verdict was WARN".to_string(),
                    Some(Verdict::Block) => "verify verdict was BLOCK".to_string(),
                    Some(Verdict::Pass) => unreachable!("matched above"),
                    None => "no verify verdict found in the phase's final message".to_string(),
                };
                warn!("queue_run: phase {execution_id} stopped for {path}: {reason}");
                plan.phases[idx].status = RunPhaseStatus::Failed;
                plan.phases[idx].park_payload = Some(reason);
                runplan::save(root, plan)?;
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn register_and_deregister_run_guards_reentrancy() {
        let state = AppState {
            config: std::sync::Mutex::new(crate::config::GlobalConfig::default()),
            claude_sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending_answers: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending_permissions: std::sync::Mutex::new(std::collections::HashMap::new()),
            pending_plans: std::sync::Mutex::new(std::collections::HashMap::new()),
            interrupt_slots: std::sync::Mutex::new(std::collections::HashMap::new()),
            run_cancel: std::sync::Mutex::new(std::collections::HashMap::new()),
        };
        let root = PathBuf::from("/repo");

        let _flag = register_run(&state, &root).expect("first registration succeeds");
        assert!(register_run(&state, &root).is_err(), "second is rejected");

        deregister_run(&state, &root);
        assert!(
            register_run(&state, &root).is_ok(),
            "slot is free after deregister"
        );
    }
}
