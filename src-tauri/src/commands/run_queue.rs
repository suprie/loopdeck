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

use super::agent::start_fresh_and_record;
use super::state::{fire_interrupt, resolve_root, AppState};
use crate::epic;
use crate::error::AppError;
use crate::execution::{self, LoopOrigin};
use crate::run_executor::{self, build_phase_prompt, extract_verdict, RunHandle, RunVerdict};
use crate::runplan::{self, RunPhaseStatus, RunPlan};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tracing::{info, warn};

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

/// Cancel the in-progress run for a project. Fires the run's cancel flag
/// (checked between phases) and, since the executor may be mid-turn, also
/// interrupts the live session the same way a user-initiated Stop would —
/// cancel takes effect immediately rather than waiting for the current
/// phase's turn to finish on its own. No-op error if no run is active.
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

/// The sequential executor loop: one orchestrated turn per queued phase, in
/// the plan's authored (vec) order.
///
/// **Phase 2 scope** — no dependency-graph skip and no interactive-stall
/// parking yet (`depends_on`/`StallPolicy` are recorded but not consulted;
/// Phase 4 wires that in). A non-green verdict or a turn-level error stops
/// the run at that phase rather than trying the next one, since without
/// Phase 4's stall-vs-failure distinction, continuing past an unresolved
/// phase would let a later phase build on work its dependency didn't
/// actually finish.
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

        let prompt = build_phase_prompt(&execution_id, &loc, &interview);
        let outcome = start_fresh_and_record(state, root, &prompt).await;

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
