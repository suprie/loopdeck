//! Dependency-aware phase scheduler — `prd-parallel-scheduling`.
//!
//! Pure scheduling policy for run-plan phases: graph validation (no cycles,
//! no dangling edges), the Waiting gate that holds dependents until upstream
//! phases land green, the ready-set the executor draws from, dependent
//! failure propagation, and the per-phase watchdog budget. No filesystem,
//! Tauri, or clock dependencies — the executor owns persistence and the
//! actual turns, so every rule here is unit-testable.
//!
//! Design (PRD Design stub): a **ready-set with a semaphore**, not
//! topological batches. The executor asks which phases are ready, starts up
//! to `RunPlan::max_concurrency` of them, and re-evaluates whenever one
//! finishes. That lets a fast phase's dependents start while a slow sibling
//! on another branch is still running.

use crate::error::AppError;
use crate::runplan::{RunPhase, RunPhaseStatus, RunPlan, StallPolicy};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A phase counts as green only when it actually delivered.
pub fn is_success(status: RunPhaseStatus) -> bool {
    matches!(
        status,
        RunPhaseStatus::Completed | RunPhaseStatus::Delivered
    )
}

/// A phase will never change status again on its own.
pub fn is_terminal(status: RunPhaseStatus) -> bool {
    matches!(
        status,
        RunPhaseStatus::Completed
            | RunPhaseStatus::Delivered
            | RunPhaseStatus::Parked
            | RunPhaseStatus::Failed
            | RunPhaseStatus::Interrupted
            | RunPhaseStatus::Killed
    )
}

/// Whether a phase still needs the scheduler's attention.
pub fn is_active(status: RunPhaseStatus) -> bool {
    matches!(
        status,
        RunPhaseStatus::Queued | RunPhaseStatus::Waiting | RunPhaseStatus::Running
    )
}

fn status_label(status: RunPhaseStatus) -> &'static str {
    match status {
        RunPhaseStatus::Queued => "queued",
        RunPhaseStatus::Waiting => "waiting",
        RunPhaseStatus::Running => "running",
        RunPhaseStatus::Parked => "parked",
        RunPhaseStatus::Delivered => "delivered",
        RunPhaseStatus::Completed => "completed",
        RunPhaseStatus::Failed => "failed",
        RunPhaseStatus::Interrupted => "interrupted",
        RunPhaseStatus::Killed => "killed",
    }
}

/// Validate a plan's dependency graph before any turn starts. A hand-edited
/// plan (or a future edge editor) must never reach the executor with a
/// dangling, self-referential, or cyclic edge: a cycle would leave every
/// member Waiting forever, quietly wasting the overnight window.
pub fn validate_dependency_graph(phases: &[RunPhase]) -> Result<(), AppError> {
    let mut seen = HashSet::new();
    for phase in phases {
        if phase.execution_id.trim().is_empty() {
            return Err(AppError::RunPlan(
                "every run-plan phase needs a non-empty execution id".into(),
            ));
        }
        if !seen.insert(phase.execution_id.as_str()) {
            return Err(AppError::RunPlan(format!(
                "phase \"{}\" appears more than once in the plan",
                phase.execution_id
            )));
        }
    }
    for phase in phases {
        for dependency in &phase.depends_on {
            if dependency == &phase.execution_id {
                return Err(AppError::RunPlan(format!(
                    "phase \"{}\" depends on itself",
                    phase.execution_id
                )));
            }
            if !seen.contains(dependency.as_str()) {
                return Err(AppError::RunPlan(format!(
                    "phase \"{}\" depends on \"{dependency}\", which is not part of this plan",
                    phase.execution_id
                )));
            }
        }
    }
    topological_order(phases).map(|_| ())
}

/// Deterministic topological order (authored order breaks ties). Errors with
/// the cycle members when the graph has a cycle — this is the single cycle
/// detector both plan creation and run start use.
pub fn topological_order(phases: &[RunPhase]) -> Result<Vec<String>, AppError> {
    let mut remaining: Vec<usize> = (0..phases.len()).collect();
    let mut resolved: HashSet<&str> = HashSet::new();
    let mut order = Vec::with_capacity(phases.len());
    while !remaining.is_empty() {
        let before = remaining.len();
        let mut next = Vec::new();
        // Only nodes whose dependencies were already resolved *before* this
        // pass may resolve now. Resolving mid-pass would let a cycle resolve
        // itself whenever its back-edge happens to sit at a later index.
        let mut newly = Vec::new();
        for &index in &remaining {
            let phase = &phases[index];
            let ready = phase
                .depends_on
                .iter()
                .all(|dependency| resolved.contains(dependency.as_str()));
            if ready {
                newly.push(index);
            } else {
                next.push(index);
            }
        }
        for index in newly {
            let phase = &phases[index];
            resolved.insert(phase.execution_id.as_str());
            order.push(phase.execution_id.clone());
        }
        remaining = next;
        if remaining.len() == before {
            let members: Vec<&str> = remaining
                .iter()
                .map(|&index| phases[index].execution_id.as_str())
                .collect();
            return Err(AppError::RunPlan(format!(
                "dependency cycle in the run plan among: {}",
                members.join(", ")
            )));
        }
    }
    Ok(order)
}

/// Hold every `Queued` phase with an unsatisfied dependency in `Waiting`.
/// Called once when the executor starts, so legacy plans (all phases
/// `Queued`) keep their on-disk queue shape until the run actually begins.
/// Returns how many phases were moved to Waiting.
pub fn mark_waiting(plan: &mut RunPlan) -> usize {
    let successful: HashSet<String> = plan
        .phases
        .iter()
        .filter(|phase| is_success(phase.status))
        .map(|phase| phase.execution_id.clone())
        .collect();
    let mut moved = 0;
    for phase in &mut plan.phases {
        if phase.status == RunPhaseStatus::Queued
            && phase
                .depends_on
                .iter()
                .any(|dependency| !successful.contains(dependency))
        {
            phase.status = RunPhaseStatus::Waiting;
            moved += 1;
        }
    }
    moved
}

/// Recompute the dependency gate after any phase reaches a terminal state:
///
/// - `Waiting` phases whose dependencies are all green are released to
///   `Queued`.
/// - `Waiting`/`Queued` phases with an upstream that ended non-green become
///   `Failed`, naming that upstream in `park_payload` (the approved stall
///   answer: *fail dependents, keep going*).
///
/// Runs to a fixed point so a failed phase can fail its own dependents in the
/// same pass. Never touches `Running` phases: an independent in-flight phase
/// always finishes on its own. Returns `(released, failed)` counts.
pub fn reconcile_gates(plan: &mut RunPlan) -> (usize, usize) {
    let mut released = 0;
    let mut failed = 0;
    // At most one pass per phase; a wider bound cannot loop forever.
    for _ in 0..=plan.phases.len() {
        let snapshot: Vec<(String, RunPhaseStatus, Option<String>)> = plan
            .phases
            .iter()
            .map(|phase| {
                (
                    phase.execution_id.clone(),
                    phase.status,
                    phase.park_payload.clone(),
                )
            })
            .collect();
        let mut changes: Vec<(usize, RunPhaseStatus, Option<String>)> = Vec::new();
        for (index, phase) in plan.phases.iter().enumerate() {
            if !matches!(
                phase.status,
                RunPhaseStatus::Queued | RunPhaseStatus::Waiting
            ) {
                continue;
            }
            let mut all_green = true;
            let mut failure: Option<(String, String)> = None;
            for dependency in &phase.depends_on {
                let Some((upstream_id, status, payload)) = snapshot
                    .iter()
                    .find(|(candidate_id, _, _)| candidate_id == dependency)
                else {
                    // Validation rejects dangling edges; treat a missing edge
                    // defensively as not-yet-green rather than running early.
                    all_green = false;
                    continue;
                };
                if is_success(*status) {
                    continue;
                }
                all_green = false;
                if is_terminal(*status) {
                    let label = status_label(*status).to_string();
                    failure = Some((
                        upstream_id.clone(),
                        match payload.as_deref() {
                            Some(reason) if !reason.is_empty() => format!("{label}: {reason}"),
                            _ => label,
                        },
                    ));
                }
            }
            if let Some((upstream_id, detail)) = failure {
                changes.push((
                    index,
                    RunPhaseStatus::Failed,
                    Some(format!(
                        "failed: upstream phase \"{upstream_id}\" did not complete green ({detail})"
                    )),
                ));
            } else if all_green && phase.status == RunPhaseStatus::Waiting {
                changes.push((index, RunPhaseStatus::Queued, None));
            }
        }
        if changes.is_empty() {
            break;
        }
        for (index, status, payload) in changes {
            plan.phases[index].status = status;
            if status == RunPhaseStatus::Failed {
                failed += 1;
            } else if status == RunPhaseStatus::Queued {
                released += 1;
            }
            plan.phases[index].park_payload = payload;
        }
    }
    (released, failed)
}

/// Mark every not-yet-started dependent of `failed_id` as `Failed`, with the
/// upstream reason. `StallPolicy::Halt` additionally parks every remaining
/// `Queued`/`Waiting` phase that is not already failed, preserving strict
/// sequence. `Running` phases are never touched — the approved gated-start
/// answer keeps independent in-flight work going.
///
/// Returns the number of phases whose status changed.
pub fn fail_dependents(plan: &mut RunPlan, failed_id: &str, reason: &str) -> usize {
    let affected: HashSet<String> = dependents_of(plan, failed_id).into_iter().collect();

    let halt = plan.stall_policy == StallPolicy::Halt;
    let mut changed = 0;
    for phase in &mut plan.phases {
        if !matches!(
            phase.status,
            RunPhaseStatus::Queued | RunPhaseStatus::Waiting
        ) {
            continue;
        }
        if affected.contains(&phase.execution_id) {
            phase.status = RunPhaseStatus::Failed;
            phase.park_payload = Some(format!(
                "failed: upstream phase \"{failed_id}\" did not complete green ({reason})"
            ));
            changed += 1;
        } else if halt {
            phase.status = RunPhaseStatus::Parked;
            phase.park_payload = Some(format!(
                "halted: run stopped after phase \"{failed_id}\": {reason}"
            ));
            changed += 1;
        }
    }
    changed
}

/// The phases the executor may start right now, in authored order, capped at
/// `limit`. A phase is ready when it is `Queued`, not already in flight, and
/// every dependency is green.
pub fn select_ready(plan: &RunPlan, in_flight: &HashSet<String>, limit: usize) -> Vec<usize> {
    if limit == 0 {
        return Vec::new();
    }
    let successful: HashSet<&str> = plan
        .phases
        .iter()
        .filter(|phase| is_success(phase.status))
        .map(|phase| phase.execution_id.as_str())
        .collect();
    plan.phases
        .iter()
        .enumerate()
        .filter(|(_, phase)| phase.status == RunPhaseStatus::Queued)
        .filter(|(_, phase)| !in_flight.contains(&phase.execution_id))
        .filter(|(_, phase)| {
            phase
                .depends_on
                .iter()
                .all(|dependency| successful.contains(dependency.as_str()))
        })
        .map(|(index, _)| index)
        .take(limit)
        .collect()
}

/// Per-phase watchdog: whichever cap expires first — this phase's own
/// wall-clock budget or the run's remaining wall-clock budget. Returns the
/// timeout and the reason to record when it fires.
pub fn phase_watchdog(
    phase_timeout: Duration,
    run_remaining: Duration,
) -> (Duration, &'static str) {
    if run_remaining <= phase_timeout {
        (run_remaining, "total run wall-clock budget exceeded")
    } else {
        (phase_timeout, "phase wall-clock budget exceeded")
    }
}

/// Restart-reconciliation decision for one phase left `Running` on disk.
/// Auto-resume requires both survivors: the worktree the phase ran in and the
/// saved provider session. Anything less parks the phase with a reason for
/// the morning report instead of guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeDecision {
    AutoResume,
    Park,
}

pub fn classify_interrupted_phase(
    worktree_survives: bool,
    session_survives: bool,
) -> ResumeDecision {
    if worktree_survives && session_survives {
        ResumeDecision::AutoResume
    } else {
        ResumeDecision::Park
    }
}

/// Deterministic per-phase branch name. Every phase branch is cut from the
/// repository default branch — no stacking — and carries its authoured index
/// so two different execution ids that normalize to the same slug can never
/// collide.
pub fn phase_branch_name(plan_id: &str, execution_id: &str, index: usize) -> String {
    let suffix = plan_id.rsplit('-').next().unwrap_or(plan_id);
    let slug: String = execution_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() {
        "phase".to_string()
    } else {
        slug
    };
    format!("run/{slug}-p{}-{suffix}", index + 1)
}

/// Managed worktree path for a phase branch (inside `.loopdeck/runs/`).
pub fn phase_worktree_path(root: &Path, branch: &str) -> PathBuf {
    root.join(".loopdeck").join("runs").join(branch)
}

/// A plan is schedulable while any phase is still Queued, Waiting, or
/// Running. Used by the executor loop's termination check and by `queue_run`.
pub fn has_active_phases(plan: &RunPlan) -> bool {
    plan.phases.iter().any(|phase| is_active(phase.status))
}

/// Breadth-first set of phases that transitively depend on `execution_id`
/// (excluding itself). Exposed for the morning report and tests.
pub fn dependents_of(plan: &RunPlan, execution_id: &str) -> Vec<String> {
    let mut affected: HashSet<String> = HashSet::from([execution_id.to_string()]);
    let mut queue = VecDeque::from([execution_id.to_string()]);
    while let Some(current) = queue.pop_front() {
        for phase in &plan.phases {
            if phase
                .depends_on
                .iter()
                .any(|dependency| dependency == &current)
                && affected.insert(phase.execution_id.clone())
            {
                queue.push_back(phase.execution_id.clone());
            }
        }
    }
    affected.remove(execution_id);
    let mut result: Vec<String> = affected.into_iter().collect();
    result.sort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runplan::{RunBudgets, RunConsent, RunPhase};
    use chrono::Utc;

    fn phase(id: &str, depends_on: &[&str]) -> RunPhase {
        RunPhase {
            execution_id: id.to_string(),
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            ..Default::default()
        }
    }

    fn plan(phases: Vec<RunPhase>) -> RunPlan {
        RunPlan {
            id: "run-abc12345".into(),
            project: PathBuf::from("/repo"),
            created: Utc::now(),
            consent: RunConsent::default(),
            budgets: RunBudgets::default(),
            environment: Default::default(),
            wall_clock_secs: 0,
            stall_policy: StallPolicy::ContinueIndependent,
            max_concurrency: 4,
            phases,
        }
    }

    fn diamond() -> RunPlan {
        // a → b, a → c, (b, c) → d
        plan(vec![
            phase("a", &[]),
            phase("b", &["a"]),
            phase("c", &["a"]),
            phase("d", &["b", "c"]),
        ])
    }

    fn set_status(plan: &mut RunPlan, id: &str, status: RunPhaseStatus) {
        plan.phases
            .iter_mut()
            .find(|phase| phase.execution_id == id)
            .expect("phase exists")
            .status = status;
    }

    #[test]
    fn diamond_graph_validates_and_orders_roots_first() {
        let mut plan = diamond();
        validate_dependency_graph(&plan.phases).expect("diamond is acyclic");
        let order = topological_order(&plan.phases).expect("order");
        assert_eq!(order, vec!["a", "b", "c", "d"]);

        // Only the root is ready before anything lands.
        mark_waiting(&mut plan);
        assert_eq!(plan.phases[0].status, RunPhaseStatus::Queued);
        assert!(plan.phases[1..]
            .iter()
            .all(|phase| phase.status == RunPhaseStatus::Waiting));
        let ready = select_ready(&plan, &HashSet::new(), 4);
        assert_eq!(ready, vec![0]);
    }

    #[test]
    fn diamond_releases_a_leaf_only_when_both_parents_are_green() {
        let mut plan = diamond();
        mark_waiting(&mut plan);
        set_status(&mut plan, "a", RunPhaseStatus::Completed);

        let (released, failed) = reconcile_gates(&mut plan);
        assert_eq!((released, failed), (2, 0));
        assert_eq!(plan.phases[1].status, RunPhaseStatus::Queued);
        assert_eq!(plan.phases[2].status, RunPhaseStatus::Queued);
        assert_eq!(plan.phases[3].status, RunPhaseStatus::Waiting);

        set_status(&mut plan, "b", RunPhaseStatus::Completed);
        let (released, _) = reconcile_gates(&mut plan);
        assert_eq!(released, 0);
        assert_eq!(plan.phases[3].status, RunPhaseStatus::Waiting);

        set_status(&mut plan, "c", RunPhaseStatus::Delivered);
        let (released, _) = reconcile_gates(&mut plan);
        assert_eq!(released, 1);
        assert_eq!(plan.phases[3].status, RunPhaseStatus::Queued);
    }

    #[test]
    fn non_green_upstream_fails_every_waiting_dependent_transitively() {
        let mut plan = diamond();
        mark_waiting(&mut plan);
        set_status(&mut plan, "a", RunPhaseStatus::Completed);
        reconcile_gates(&mut plan);
        set_status(&mut plan, "b", RunPhaseStatus::Parked);
        plan.phases[1].park_payload = Some("verify verdict: BLOCK".into());

        let (_, failed) = reconcile_gates(&mut plan);
        assert_eq!(failed, 1);
        let d = &plan.phases[3];
        assert_eq!(d.status, RunPhaseStatus::Failed);
        let reason = d.park_payload.as_deref().expect("reason");
        assert!(reason.contains("upstream phase \"b\""), "{reason}");
        assert!(reason.contains("verify verdict: BLOCK"), "{reason}");
        // The independent sibling c is untouched.
        assert_eq!(plan.phases[2].status, RunPhaseStatus::Queued);
    }

    #[test]
    fn fail_dependents_keeps_independent_in_flight_phases_running() {
        let mut plan = plan(vec![phase("a", &[]), phase("b", &["a"]), phase("c", &[])]);
        set_status(&mut plan, "a", RunPhaseStatus::Killed);
        set_status(&mut plan, "b", RunPhaseStatus::Waiting);
        set_status(&mut plan, "c", RunPhaseStatus::Running);

        let changed = fail_dependents(&mut plan, "a", "phase token budget exceeded");
        assert_eq!(changed, 1);
        assert_eq!(plan.phases[1].status, RunPhaseStatus::Failed);
        assert!(plan.phases[1]
            .park_payload
            .as_deref()
            .unwrap()
            .contains("phase token budget exceeded"));
        // A budget kill ends only that phase; the in-flight sibling keeps going.
        assert_eq!(plan.phases[2].status, RunPhaseStatus::Running);
    }

    #[test]
    fn halt_policy_parks_every_remaining_phase() {
        let mut plan = plan(vec![phase("a", &[]), phase("b", &["a"]), phase("c", &[])]);
        plan.stall_policy = StallPolicy::Halt;
        set_status(&mut plan, "a", RunPhaseStatus::Failed);
        set_status(&mut plan, "b", RunPhaseStatus::Waiting);
        set_status(&mut plan, "c", RunPhaseStatus::Queued);

        fail_dependents(&mut plan, "a", "turn error");
        assert_eq!(plan.phases[1].status, RunPhaseStatus::Failed);
        assert_eq!(plan.phases[2].status, RunPhaseStatus::Parked);
        assert!(plan.phases[2]
            .park_payload
            .as_deref()
            .unwrap()
            .starts_with("halted:"));
    }

    #[test]
    fn cycle_is_rejected_and_names_the_members() {
        let cyclic = plan(vec![
            phase("a", &["c"]),
            phase("b", &["a"]),
            phase("c", &["b"]),
        ]);
        let error = validate_dependency_graph(&cyclic.phases).expect_err("cycle rejected");
        let message = error.to_string();
        assert!(message.contains("cycle"), "{message}");
        assert!(message.contains('a') && message.contains('b') && message.contains('c'));
    }

    #[test]
    fn self_dependency_and_dangling_edges_are_rejected() {
        let self_dep = plan(vec![phase("a", &["a"])]);
        assert!(validate_dependency_graph(&self_dep.phases)
            .expect_err("self dependency")
            .to_string()
            .contains("depends on itself"));

        let dangling = plan(vec![phase("a", &["ghost"])]);
        assert!(validate_dependency_graph(&dangling.phases)
            .expect_err("dangling")
            .to_string()
            .contains("not part of this plan"));
    }

    #[test]
    fn select_ready_respects_the_concurrency_limit() {
        let plan = plan(vec![phase("a", &[]), phase("b", &[]), phase("c", &[])]);
        assert_eq!(select_ready(&plan, &HashSet::new(), 2), vec![0, 1]);
        let in_flight = HashSet::from(["a".to_string()]);
        assert_eq!(select_ready(&plan, &in_flight, 4), vec![1, 2]);
    }

    #[test]
    fn watchdog_picks_the_nearer_cap() {
        let phase = Duration::from_secs(90);
        let run = Duration::from_secs(30);
        assert_eq!(
            phase_watchdog(phase, run),
            (run, "total run wall-clock budget exceeded")
        );
        assert_eq!(
            phase_watchdog(phase, Duration::from_secs(300)),
            (phase, "phase wall-clock budget exceeded")
        );
    }

    #[test]
    fn restart_decision_needs_both_worktree_and_session() {
        assert_eq!(
            classify_interrupted_phase(true, true),
            ResumeDecision::AutoResume
        );
        assert_eq!(
            classify_interrupted_phase(true, false),
            ResumeDecision::Park
        );
        assert_eq!(
            classify_interrupted_phase(false, true),
            ResumeDecision::Park
        );
    }

    #[test]
    fn phase_branches_are_unique_even_when_slugs_collide() {
        let first = phase_branch_name("run-abc123", "prd-x/loop-a", 0);
        let second = phase_branch_name("run-abc123", "prd-x-loop-a", 1);
        assert_ne!(first, second);
        assert!(first.starts_with("run/prd-x-loop-a-p1-"));
        assert!(second.starts_with("run/prd-x-loop-a-p2-"));
    }

    #[test]
    fn dependents_of_walks_the_whole_chain() {
        let plan = plan(vec![
            phase("a", &[]),
            phase("b", &["a"]),
            phase("c", &["b"]),
            phase("d", &[]),
        ]);
        assert_eq!(dependents_of(&plan, "a"), vec!["b", "c"]);
        assert!(dependents_of(&plan, "d").is_empty());
    }

    #[test]
    fn mark_waiting_leaves_roots_and_completed_phases_alone() {
        let mut plan = diamond();
        plan.phases[0].status = RunPhaseStatus::Queued;
        plan.phases[2].status = RunPhaseStatus::Completed;
        let moved = mark_waiting(&mut plan);
        assert_eq!(moved, 2); // b and d; c is already completed
        assert_eq!(plan.phases[0].status, RunPhaseStatus::Queued);
        assert_eq!(plan.phases[2].status, RunPhaseStatus::Completed);
        assert_eq!(plan.phases[1].status, RunPhaseStatus::Waiting);
        assert_eq!(plan.phases[3].status, RunPhaseStatus::Waiting);
    }

    // ── Property tests over generated graphs (`parallel-scheduling/tests`) ──
    //
    // A deterministic 64-bit LCG keeps the "random" graphs reproducible in CI
    // without adding a property-test dependency. Each case is a real graph
    // property, not a snapshot: every generated DAG must validate, and the
    // ready-set simulation must eventually green every phase without ever
    // starting one before its dependencies.

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        fn below(&mut self, bound: u64) -> u64 {
            self.next() % bound
        }
    }

    /// Generate a DAG: phase `i` may depend only on phases with a lower
    /// index, which makes a cycle impossible by construction.
    fn random_dag(seed: u64, size: usize) -> RunPlan {
        let mut rng = Lcg(seed);
        let ids: Vec<String> = (0..size).map(|index| format!("p{index}")).collect();
        let phases = ids
            .iter()
            .enumerate()
            .map(|(index, id)| {
                let mut deps = Vec::new();
                for candidate in 0..index {
                    if rng.below(4) == 0 {
                        deps.push(ids[candidate].clone());
                    }
                }
                RunPhase {
                    execution_id: id.clone(),
                    depends_on: deps,
                    ..Default::default()
                }
            })
            .collect();
        plan(phases)
    }

    #[test]
    fn property_every_generated_dag_validates_and_runs_to_green() {
        for seed in 0..64 {
            let mut plan = random_dag(seed, 8);
            validate_dependency_graph(&plan.phases)
                .unwrap_or_else(|error| panic!("seed {seed}: {error}"));
            mark_waiting(&mut plan);
            let mut in_flight: HashSet<String> = HashSet::new();
            let mut executed = 0;
            // Simulate the executor with a concurrency of three.
            for _ in 0..(plan.phases.len() * 4) {
                let ready = select_ready(&plan, &in_flight, 3);
                for start in ready {
                    let phase = &plan.phases[start];
                    for dependency in &phase.depends_on {
                        let upstream = plan
                            .phases
                            .iter()
                            .find(|candidate| &candidate.execution_id == dependency)
                            .expect("dependency exists");
                        assert!(
                            is_success(upstream.status),
                            "seed {seed}: started {} before {dependency} was green",
                            phase.execution_id
                        );
                    }
                    in_flight.insert(phase.execution_id.clone());
                }
                // Complete one in-flight phase per tick.
                if let Some(completed) = in_flight.iter().next().cloned() {
                    in_flight.remove(&completed);
                    let index = plan
                        .phases
                        .iter()
                        .position(|candidate| candidate.execution_id == completed)
                        .expect("in flight phase exists");
                    plan.phases[index].status = RunPhaseStatus::Completed;
                    executed += 1;
                    reconcile_gates(&mut plan);
                } else if !has_active_phases(&plan) {
                    break;
                }
            }
            assert_eq!(
                executed,
                plan.phases.len(),
                "seed {seed}: every phase must run exactly once"
            );
            assert!(plan.phases.iter().all(|phase| is_success(phase.status)));
        }
    }

    #[test]
    fn property_every_generated_cycle_is_rejected() {
        // Add one back-edge to an acyclic graph: a cycle must always be
        // caught, whichever edge closes it.
        for seed in 0..32 {
            let mut plan = random_dag(seed, 6);
            let size = plan.phases.len();
            // Force a chain so the back-edge below is guaranteed to close a
            // cycle through the whole graph.
            for index in 1..size {
                let previous = plan.phases[index - 1].execution_id.clone();
                plan.phases[index].depends_on.push(previous);
            }
            // The head depends on the tail: every node is now on the cycle.
            let last_id = plan.phases[size - 1].execution_id.clone();
            plan.phases[0].depends_on.push(last_id);
            let error = validate_dependency_graph(&plan.phases)
                .expect_err("a back-edge or self-edge must be rejected");
            let message = error.to_string();
            assert!(
                message.contains("cycle") || message.contains("depends on itself"),
                "seed {seed}: unexpected rejection reason: {message}"
            );
        }
    }

    #[test]
    fn property_restart_decisions_cover_all_survivor_combinations() {
        for worktree in [false, true] {
            for session in [false, true] {
                let expected = if worktree && session {
                    ResumeDecision::AutoResume
                } else {
                    ResumeDecision::Park
                };
                assert_eq!(
                    classify_interrupted_phase(worktree, session),
                    expected,
                    "worktree={worktree} session={session}"
                );
            }
        }
    }

    #[test]
    fn has_active_phases_follows_terminal_states() {
        let mut plan = diamond();
        assert!(has_active_phases(&plan));
        for phase in &mut plan.phases {
            phase.status = RunPhaseStatus::Completed;
        }
        assert!(!has_active_phases(&plan));
    }
}
