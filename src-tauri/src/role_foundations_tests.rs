//! `prd-role-foundations` Phase 5 — focused coverage for the three joins the
//! PRD introduces: charter migration, charter injection routing, and
//! per-phase assignment.
//!
//! The spawn-level behavior already has fixtures: `charter_injection_tests`
//! captures the fake child's argv for the interactive, run-queue, and
//! multi-agent paths, and `role_assignment_tests` walks the two-role demo
//! through the executor's staffing seam. This module covers the seams those
//! suites do not: loading a legacy file from disk, the resolver both spawn
//! paths share, and assignment persistence plus its batching consequence.
//!
//! No live providers — every test here is deterministic and local.

use crate::charter_injection_tests::empty_state;
use crate::commands::state::{resolve_agent_config, resolve_agent_config_by_id};
use crate::config::{AgentConfig, GlobalConfig, NamedAgentConfig, RoleCharter};
use crate::run_executor::build_run_plan;
use crate::runplan::{self, PhaseAgentAssignment, StallPolicy};
use std::path::PathBuf;

fn dev_charter() -> RoleCharter {
    RoleCharter {
        persona_prompt: Some("You are the dev-role agent. You build.".into()),
        allowed_skills: Some(vec!["loopdeck-orchestrator".into()]),
        output_contract: Some("End with a Verdict line.".into()),
        rules: None,
    }
}

fn qa_charter() -> RoleCharter {
    RoleCharter {
        persona_prompt: Some("You are the QA-role agent. You verify.".into()),
        allowed_skills: Some(vec!["loopdeck-prd-verifier".into()]),
        output_contract: Some("End with a Verdict line.".into()),
        rules: None,
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "loopdeck-role-foundations-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Migration (ADR-3) at the disk seam: a `config.yaml` written before charter
/// fields existed loads without a charter and is resaved without growing one.
#[test]
fn legacy_config_file_loads_and_resaves_without_a_charter() {
    let dir = temp_dir("legacy-config");
    let path = dir.join("config.yaml");
    let legacy =
        "agents:\n  - id: 00000000-0000-4000-8000-000000000001\n    name: Default\n    model: m1\n";
    std::fs::write(&path, legacy).unwrap();

    let config = GlobalConfig::load_from_path(&path).expect("legacy registry loads");
    let agent = config.agents.first().expect("one roster entry");
    assert!(agent.charter.is_empty());
    assert_eq!(agent.charter, RoleCharter::default());

    config.save_to_path(&path).expect("resave");
    let resaved = std::fs::read_to_string(&path).unwrap();
    assert!(
        !resaved.contains("charter"),
        "migration must not add charter keys:\n{resaved}"
    );
    assert_eq!(
        GlobalConfig::load_from_path(&path)
            .unwrap()
            .agents
            .first()
            .unwrap()
            .charter,
        RoleCharter::default(),
        "the pre-charter entry must stay a plain connection profile"
    );

    std::fs::remove_dir_all(dir).ok();
}

/// The interactive and run-queue paths both resolve the default entry; the
/// multi-agent and assigned-phase paths resolve by id. Both must carry the
/// same charter into the spawn config without cross-role leakage.
#[test]
fn default_and_assigned_resolvers_carry_only_their_roles_charter() {
    let state = empty_state();
    let mut dev = NamedAgentConfig::new("Dev".into(), AgentConfig::default()).unwrap();
    let mut qa = NamedAgentConfig::new("QA".into(), AgentConfig::default()).unwrap();
    dev.charter = dev_charter();
    qa.charter = qa_charter();
    let (dev_id, qa_id) = (dev.id.clone(), qa.id.clone());
    {
        let mut config = state.config.lock().unwrap();
        config.default_agent_id = Some(dev_id.clone());
        config.agents.push(dev);
        config.agents.push(qa);
    }

    let by_default = resolve_agent_config(&state).expect("default resolves");
    let by_id = resolve_agent_config_by_id(&state, &dev_id).expect("assigned id resolves");
    assert_eq!(by_default.charter, Some(dev_charter()));
    assert_eq!(
        by_default.charter, by_id.charter,
        "both spawn paths must receive the same charter carrier"
    );

    let qa_config = resolve_agent_config_by_id(&state, &qa_id).expect("qa resolves");
    let rendered = qa_config.charter.as_ref().unwrap().render();
    assert!(rendered.contains("QA-role"));
    assert!(
        !rendered.contains("dev-role"),
        "one role's charter must never leak into another's spawn:\n{rendered}"
    );
}

/// A pre-assignment `run-plan.yaml` loads unchanged: the new assignment and
/// handoff fields default to `None`, so old plans keep running on the default
/// agent.
#[test]
fn legacy_run_plan_without_assignment_fields_keeps_the_default_agent() {
    let dir = temp_dir("legacy-plan");
    let path = dir.join("run-plan.yaml");
    std::fs::write(
        &path,
        "id: run-legacy\nproject: /repo\ncreated: 2026-07-28T09:00:00Z\nphases:\n  - execution_id: demo/one\n  - execution_id: demo/two\n",
    )
    .unwrap();

    let plan = runplan::load_from_path(&path)
        .expect("legacy plan loads")
        .expect("plan present");
    for phase in &plan.phases {
        assert!(phase.assigned_agent.is_none());
        assert!(phase.assigned_agent_id.is_none());
        assert!(phase.assigned_agent_name.is_none());
        assert!(phase.handoff_topic.is_none());
    }
    assert_eq!(
        crate::commands::run_queue::next_queued_batch(&plan),
        Some(vec![0, 1]),
        "all-unassigned phases still share one combined turn"
    );

    std::fs::remove_dir_all(dir).ok();
}

/// Assignment plus its artifact contract survive the plan file, and the
/// persisted staffing split still decides the executor's batch.
#[test]
fn assignment_and_handoff_topic_round_trip_through_the_plan_file() {
    let dir = temp_dir("assigned-plan");
    let assignments = vec![
        PhaseAgentAssignment {
            execution_id: "demo/dev-build".into(),
            agent_id: "dev-uuid".into(),
            agent_name: Some("Dev".into()),
        },
        PhaseAgentAssignment {
            execution_id: "demo/qa-verify".into(),
            agent_id: "qa-uuid".into(),
            agent_name: Some("QA".into()),
        },
    ];
    let mut plan = build_run_plan(
        "run-assigned".into(),
        dir.clone(),
        chrono::Utc::now(),
        &["demo/dev-build".into(), "demo/qa-verify".into()],
        StallPolicy::Halt,
        false,
        &assignments,
    );
    plan.phases[0].handoff_topic = Some("demo-dev-build".into());
    plan.phases[1].handoff_topic = Some("demo-qa-verify".into());

    runplan::save(&dir, &plan).expect("plan saves");
    let loaded = runplan::load(&dir).expect("plan loads").expect("present");
    assert_eq!(loaded, plan);
    assert_eq!(loaded.phases[0].assigned_agent_name.as_deref(), Some("Dev"));
    assert_eq!(
        loaded.phases[1].handoff_topic.as_deref(),
        Some("demo-qa-verify")
    );
    // dev builds, QA verifies: the staffing change splits the combined turn.
    assert_eq!(
        crate::commands::run_queue::next_queued_batch(&loaded),
        Some(vec![0])
    );

    std::fs::remove_dir_all(dir).ok();
}
