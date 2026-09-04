//! End-to-end two-phase role demo (`prd-role-foundations` Phase 4, loop
//! `role-foundations/role-demo`): a dev-role agent builds, a QA-role agent
//! verifies, and the run report attributes each phase to its role.
//!
//! Deterministic harness (per the run's clarification — no live provider):
//! the fake child binaries from `charter_injection_tests` capture each
//! spawn's argv, so the demo walks the executor's real seams — queue-time
//! staffing on the plan, the staffing batch split, per-batch config
//! resolution through `resolve_agent_config_by_id`, charter injection into
//! the spawned session, and the persisted per-role attribution in the run
//! report read model.

use crate::charter_injection_tests::{empty_state, fresh_project_dir, read_capture};
use crate::commands::agent::spawn_fresh_with_config;
use crate::commands::state::resolve_agent_config_by_id;
use crate::config::{AgentConfig, NamedAgentConfig, RoleCharter};
use crate::run_executor::build_run_plan;
use crate::runplan::{AuditSlice, PhaseAgentAssignment, RunPhaseStatus, RunReport, StallPolicy};

fn dev_charter() -> RoleCharter {
    RoleCharter {
        persona_prompt: Some(
            "You are the dev-role agent. You build the change; you do not verify it.".into(),
        ),
        allowed_skills: Some(vec!["loopdeck-orchestrator".into()]),
        output_contract: Some(
            "End every final message with a Verdict line: PASS, WARN, or BLOCK.".into(),
        ),
        rules: None,
    }
}

fn qa_charter() -> RoleCharter {
    RoleCharter {
        persona_prompt: Some(
            "You are the QA-role agent. You verify work; you do not build.".into(),
        ),
        allowed_skills: Some(vec!["loopdeck-prd-verifier".into()]),
        output_contract: Some(
            "End every final message with a Verdict line: PASS, WARN, or BLOCK.".into(),
        ),
        rules: None,
    }
}

/// Roster with the two staffed roles, in the shape `create_run_plan`
/// validated them into the plan.
struct TwoRoleRoster {
    state: crate::commands::state::AppState,
    dev_id: String,
    qa_id: String,
}

fn two_role_roster() -> TwoRoleRoster {
    let state = empty_state();
    let mut dev =
        NamedAgentConfig::new("Dev".into(), AgentConfig::default()).expect("valid named agent");
    dev.charter = dev_charter();
    let mut qa =
        NamedAgentConfig::new("QA".into(), AgentConfig::default()).expect("valid named agent");
    qa.charter = qa_charter();
    let two_role = TwoRoleRoster {
        dev_id: dev.id.clone(),
        qa_id: qa.id.clone(),
        state,
    };
    {
        let mut config = two_role.state.config.lock().unwrap();
        config.agents.push(dev);
        config.agents.push(qa);
    }
    two_role
}

/// The demo: queue a two-phase plan staffed dev→build / QA→verify, execute
/// each staffing batch through the executor's exact spawn seam, and read the
/// per-role attribution back out of the run report.
#[tokio::test]
async fn two_phase_role_demo_attributes_each_phase_to_its_role() {
    let roster = two_role_roster();
    let project = fresh_project_dir("role-demo");

    // 1. Queue the plan: dev builds, QA verifies (names as resolved from the
    //    roster by `create_run_plan`).
    let execution_ids = vec!["demo/dev-build".to_string(), "demo/qa-verify".to_string()];
    let assignments = vec![
        PhaseAgentAssignment {
            execution_id: "demo/dev-build".into(),
            agent_id: roster.dev_id.clone(),
            agent_name: Some("Dev".into()),
        },
        PhaseAgentAssignment {
            execution_id: "demo/qa-verify".into(),
            agent_id: roster.qa_id.clone(),
            agent_name: Some("QA".into()),
        },
    ];
    let mut plan = build_run_plan(
        "run-demo".into(),
        project.clone(),
        chrono::Utc::now(),
        &execution_ids,
        StallPolicy::Halt,
        false,
        &assignments,
    );
    assert_eq!(
        plan.phases[0].assigned_agent_id.as_deref(),
        Some(roster.dev_id.as_str())
    );
    assert_eq!(
        plan.phases[1].assigned_agent_id.as_deref(),
        Some(roster.qa_id.as_str())
    );

    // 2. The staffing change splits the combined turn: one batch per role.
    assert_eq!(
        crate::commands::run_queue::next_queued_batch(&plan),
        Some(vec![0])
    );

    // 3. Execute each staffing batch through the executor's exact seam:
    //    resolve the assigned roster entry, spawn with that config.
    let worktrees = [
        fresh_project_dir("role-demo-dev"),
        fresh_project_dir("role-demo-qa"),
    ];
    for (i, agent_id) in [roster.dev_id.clone(), roster.qa_id.clone()]
        .into_iter()
        .enumerate()
    {
        let config = resolve_agent_config_by_id(&roster.state, &agent_id).expect("resolve");
        let session =
            spawn_fresh_with_config(&roster.state, &worktrees[i], &project, &config, true)
                .expect("role session should spawn");
        assert_eq!(
            session.lock().await.harness(),
            crate::config::AgentHarness::Claude
        );
    }

    // Each spawned child saw its own role's charter — dev builds here, QA
    // verifies there, never crossed.
    let dev_argv = read_capture(&worktrees[0].join("claude-argv.txt"));
    let qa_argv = read_capture(&worktrees[1].join("claude-argv.txt"));
    assert!(
        dev_argv.contains("You are the dev-role agent"),
        "dev argv:\n{dev_argv}"
    );
    assert!(
        !dev_argv.contains("You are the QA-role agent"),
        "dev argv:\n{dev_argv}"
    );
    assert!(
        qa_argv.contains("You are the QA-role agent"),
        "qa argv:\n{qa_argv}"
    );
    assert!(
        !qa_argv.contains("You are the dev-role agent"),
        "qa argv:\n{qa_argv}"
    );

    // 4. Morning report attribution: each phase's row carries the role name
    //    captured at queue time, read straight from persisted plan fields.
    for phase in &mut plan.phases {
        phase.status = RunPhaseStatus::Completed;
    }
    plan.phases[0].token_usage = 300_000;
    plan.phases[1].token_usage = 120_000;
    let report = RunReport::from_plan(plan, AuditSlice::default());
    assert_eq!(report.phases[0].assigned_agent_name.as_deref(), Some("Dev"));
    assert_eq!(report.phases[1].assigned_agent_name.as_deref(), Some("QA"));
    assert_eq!(report.phases[0].verdict, crate::runplan::PhaseVerdict::Pass);
    assert_eq!(report.phases[1].verdict, crate::runplan::PhaseVerdict::Pass);
}

/// An unassigned phase stays valid and runs with the default agent — the
/// backward-compat stance the run's clarification pinned: pre-assignment
/// plans and mixed plans keep working.
#[test]
fn unassigned_phase_stays_on_the_default_agent() {
    let project = fresh_project_dir("role-unassigned");
    let plan = build_run_plan(
        "run-mixed".into(),
        project,
        chrono::Utc::now(),
        &["demo/legacy-phase".to_string()],
        StallPolicy::ContinueIndependent,
        false,
        &[],
    );
    assert!(plan.phases[0].assigned_agent_id.is_none());
    assert!(plan.phases[0].assigned_agent_name.is_none());
    // All-unassigned plans still batch into one combined turn.
    assert_eq!(
        crate::commands::run_queue::next_queued_batch(&plan),
        Some(vec![0])
    );
}

/// A stale roster reference at queue time is rejected by `create_run_plan`'s
/// validation shape: the staffing lookup mirrors it — an unknown agent id
/// must never silently fall back to the default agent's credentials.
#[test]
fn resolving_an_unknown_assigned_agent_errors_instead_of_falling_back() {
    let roster = two_role_roster();
    let error = resolve_agent_config_by_id(&roster.state, "not-a-roster-id")
        .expect_err("unknown agent must not resolve");
    assert!(
        error.to_string().contains("not found"),
        "error should name the missing agent: {error}"
    );
}
