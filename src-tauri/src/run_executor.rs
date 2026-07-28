//! Pure domain logic for the `prd-run-queue` Phase 2 executor.
//!
//! This module holds the parts of the executor that don't need `AppState` or
//! a live `claude_session` — which phase runs next, whether a turn's final
//! text closes with a green verify verdict, the prompt sent to the
//! orchestrator, and the restart-recovery rule. The actual loop that drives a
//! `claude_session` per phase lives in `commands::run_queue`, since it needs
//! `AppState` to spawn sessions and touch `execution.yaml`.

use crate::error::AppError;
use crate::runplan::{self, RunPhase, RunPhaseStatus, RunPlan};
use std::path::Path;

/// The orchestrator's Phase 6 "Verify Against PRD" roll-up
/// (`templates/skills/loopdeck-prd-verifier/SKILL.md`): any FAIL → `Block`,
/// else any PARTIAL → `Warn`, else `Pass`. Only `Pass` advances the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Warn,
    Block,
}

/// Find the roll-up verdict in a turn's final text, e.g. `**Verdict:** PASS`.
/// Searches from the end so a phase's own final report — not an earlier
/// quoted example — wins. `None` means no verdict line was found (treated the
/// same as `Block` by the executor: it can't advance on silence).
pub fn parse_verdict(text: &str) -> Option<Verdict> {
    const MARKER: &str = "**Verdict:**";
    let pos = text.rfind(MARKER)?;
    let rest = text[pos + MARKER.len()..].trim_start();
    if rest.starts_with("PASS") {
        Some(Verdict::Pass)
    } else if rest.starts_with("WARN") {
        Some(Verdict::Warn)
    } else if rest.starts_with("BLOCK") {
        Some(Verdict::Block)
    } else {
        None
    }
}

/// Index of the next phase eligible to run: `Queued` and every phase it
/// `depends_on` (by `execution_id`, defaulting to the authored predecessor
/// chain per `runplan::RunPhase`) is `Completed`. A `depends_on` id absent
/// from this plan is treated as already satisfied. Scans the whole list
/// (not just the front) so a future stall-skip (Phase 4) can reuse this
/// unchanged; today's linear default chain means the first `Queued` phase is
/// always the first blocked one too.
pub fn next_eligible_phase(plan: &RunPlan) -> Option<usize> {
    let status_of = |id: &str| {
        plan.phases
            .iter()
            .find(|p| p.execution_id == id)
            .map(|p| p.status)
    };
    plan.phases.iter().position(|phase| {
        phase.status == RunPhaseStatus::Queued
            && phase
                .depends_on
                .iter()
                .all(|dep| matches!(status_of(dep), None | Some(RunPhaseStatus::Completed)))
    })
}

/// The prompt sent to start a phase's orchestrated session: invokes the
/// on-disk `loopdeck-orchestrator` skill against this phase's stable ID, with
/// any pre-flight-interview answers (Phase 3; empty today) pinned inline so
/// the agent doesn't ask again.
pub fn build_phase_prompt(phase: &RunPhase, title: &str) -> String {
    let mut prompt = format!(
        "/loopdeck:orchestrator\n\nRun phase `{}` — \"{title}\" — to completion: implement it, \
         verify it against its PRD, and end your final message with the verify verdict line \
         (`**Verdict:** PASS|WARN|BLOCK`) so this run can advance automatically.",
        phase.execution_id,
    );
    if !phase.interview.is_empty() {
        prompt.push_str("\n\nPre-answered clarifying questions — do not ask these again:\n");
        for qa in &phase.interview {
            prompt.push_str(&format!("- Q: {}\n  A: {}\n", qa.question, qa.answer));
        }
    }
    prompt
}

/// Restart recovery (PRD Phase 2, P1): downgrade any phase left `Running` by
/// a killed/restarted process to `Interrupted` and persist. Mirrors
/// `conversation::reconcile_interrupted`'s "best-effort per project, log
/// don't fail" shape — called once per registered project at app startup.
/// Returns whether anything changed; `Ok(false)` covers both "no plan queued"
/// and "nothing was running".
pub fn reconcile_after_restart(repo_path: &Path) -> Result<bool, AppError> {
    let Some(mut plan) = runplan::load(repo_path)? else {
        return Ok(false);
    };
    let mut changed = false;
    for phase in &mut plan.phases {
        if phase.status == RunPhaseStatus::Running {
            phase.status = RunPhaseStatus::Interrupted;
            changed = true;
        }
    }
    if changed {
        runplan::save(repo_path, &plan)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runplan::PinnedAnswer;
    use chrono::Utc;
    use std::path::PathBuf;

    fn phase(execution_id: &str, status: RunPhaseStatus, depends_on: &[&str]) -> RunPhase {
        RunPhase {
            execution_id: execution_id.to_string(),
            status,
            interview: vec![],
            depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
            park_payload: None,
        }
    }

    fn plan_with(phases: Vec<RunPhase>) -> RunPlan {
        RunPlan {
            id: "run-1".into(),
            project: PathBuf::from("/repo"),
            created: Utc::now(),
            consent: Default::default(),
            budgets: Default::default(),
            stall_policy: Default::default(),
            phases,
        }
    }

    #[test]
    fn parse_verdict_finds_pass() {
        assert_eq!(
            parse_verdict("some report\n\n**Verdict:** PASS\n"),
            Some(Verdict::Pass)
        );
    }

    #[test]
    fn parse_verdict_finds_warn_and_block() {
        assert_eq!(parse_verdict("**Verdict:** WARN"), Some(Verdict::Warn));
        assert_eq!(parse_verdict("**Verdict:** BLOCK"), Some(Verdict::Block));
    }

    #[test]
    fn parse_verdict_prefers_the_last_occurrence() {
        let text = "example: **Verdict:** BLOCK\n\n...\n\nfinal report\n**Verdict:** PASS";
        assert_eq!(parse_verdict(text), Some(Verdict::Pass));
    }

    #[test]
    fn parse_verdict_none_when_absent_or_garbled() {
        assert_eq!(parse_verdict("no verdict here"), None);
        assert_eq!(parse_verdict("**Verdict:** MAYBE"), None);
    }

    #[test]
    fn next_eligible_phase_picks_first_queued_with_satisfied_deps() {
        let plan = plan_with(vec![
            phase("p1", RunPhaseStatus::Completed, &[]),
            phase("p2", RunPhaseStatus::Queued, &["p1"]),
            phase("p3", RunPhaseStatus::Queued, &["p2"]),
        ]);
        assert_eq!(next_eligible_phase(&plan), Some(1));
    }

    #[test]
    fn next_eligible_phase_blocked_by_incomplete_dependency() {
        let plan = plan_with(vec![
            phase("p1", RunPhaseStatus::Failed, &[]),
            phase("p2", RunPhaseStatus::Queued, &["p1"]),
        ]);
        assert_eq!(next_eligible_phase(&plan), None);
    }

    #[test]
    fn next_eligible_phase_none_when_all_terminal() {
        let plan = plan_with(vec![
            phase("p1", RunPhaseStatus::Completed, &[]),
            phase("p2", RunPhaseStatus::Failed, &["p1"]),
        ]);
        assert_eq!(next_eligible_phase(&plan), None);
    }

    #[test]
    fn build_phase_prompt_includes_pinned_answers() {
        let mut p = phase("prd-x/phase-1", RunPhaseStatus::Queued, &[]);
        p.interview.push(PinnedAnswer {
            question: "Which stall policy?".into(),
            answer: "halt".into(),
        });
        let prompt = build_phase_prompt(&p, "Phase 1 title");
        assert!(prompt.contains("prd-x/phase-1"));
        assert!(prompt.contains("Phase 1 title"));
        assert!(prompt.contains("Which stall policy?"));
        assert!(prompt.contains("halt"));
    }

    #[test]
    fn reconcile_after_restart_downgrades_running_to_interrupted() {
        let dir =
            std::env::temp_dir().join(format!("loopdeck-run-executor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut plan = plan_with(vec![
            phase("p1", RunPhaseStatus::Running, &[]),
            phase("p2", RunPhaseStatus::Queued, &["p1"]),
        ]);
        plan.project = dir.clone();
        runplan::save(&dir, &plan).unwrap();

        let changed = reconcile_after_restart(&dir).unwrap();
        assert!(changed);
        let reloaded = runplan::load(&dir).unwrap().unwrap();
        assert_eq!(reloaded.phases[0].status, RunPhaseStatus::Interrupted);
        assert_eq!(reloaded.phases[1].status, RunPhaseStatus::Queued);

        // Idempotent: a second pass sees nothing left running.
        assert!(!reconcile_after_restart(&dir).unwrap());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reconcile_after_restart_no_plan_is_a_noop() {
        let dir =
            std::env::temp_dir().join(format!("loopdeck-run-executor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!reconcile_after_restart(&dir).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }
}
