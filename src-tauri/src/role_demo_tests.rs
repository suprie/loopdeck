//! End-to-end two-phase role demo (`prd-role-foundations` Phase 4, loop
//! `role-foundations/role-demo`): a dev-role agent builds and a QA-role
//! agent verifies, in a scratch fixture repo, with per-role attribution in
//! the run report.
//!
//! Ignored by default — it spawns REAL claude sessions (the same
//! `resolve_agent_config_by_id` → `start_fresh_and_record_streaming_in_root_with_config`
//! pair the run-queue executor drives per batch) and temporarily adds two
//! roster entries to the real global registry, removing them after the
//! report is captured (Drop guard restores the snapshot even on panic). Run
//! explicitly:
//!
//! ```text
//! cargo test role_demo -- --ignored --nocapture
//! ```
//!
//! Deviations from a literal overnight run (recorded in the run record,
//! `docs/epics/role-based-orchestration/role-demo-run.md`, mirroring the
//! handoff-spike run's "operator deviation" convention):
//!
//! - The turn prompt is demo-authored rather than
//!   `build_combined_phase_prompt` — the production prompt instructs the
//!   verify→ship / draft-PR flow, which a remoteless scratch repo must not
//!   attempt. The per-phase assignment machinery under test (batch split by
//!   agent, roster resolution, charter-carrying spawn, plan/report
//!   attribution) is exactly the executor's.

use crate::agents::{ClaudeEvent, TokenBudget};
use crate::commands::agent::start_fresh_and_record_streaming_in_root_with_config;
use crate::commands::run_queue::next_queued_batch;
use crate::commands::state::resolve_agent_config_by_id;
use crate::commands::state::AppState;
use crate::config::{AgentConfig, GlobalConfig, NamedAgentConfig, RoleCharter};
use crate::epic;
use crate::run_executor::build_run_plan;
use crate::runplan::{self, RunPhaseStatus, StallPolicy};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tauri::ipc::Channel;

/// Restore the registry snapshot on scope exit — even on panic — so the
/// demo's temp dev/QA entries can never outlive the run.
struct RegistryRestore(Option<GlobalConfig>);

impl Drop for RegistryRestore {
    fn drop(&mut self) {
        if let Some(snapshot) = self.0.take() {
            if let Err(error) = snapshot.save() {
                eprintln!("role-demo: could not restore the agent registry: {error}");
            }
        }
    }
}

fn demo_charter(persona: &str, contract: &str) -> RoleCharter {
    RoleCharter {
        persona_prompt: Some(persona.into()),
        allowed_skills: None,
        output_contract: Some(contract.into()),
        rules: None,
    }
}

/// Scratch fixture repo: git init + its own `.loopdeck/` + one epic PRD with
/// the two authored loop specs. Nothing here touches loopdeck's real
/// docs/epics or execution.yaml.
fn fixture_repo() -> PathBuf {
    let repo = std::env::temp_dir().join(format!(
        "loopdeck-role-demo-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(repo.join(".loopdeck")).expect("create .loopdeck");
    std::fs::create_dir_all(repo.join("docs/epics/role-demo")).expect("create epic dir");

    std::fs::write(
        repo.join("docs/epics/role-demo/README.md"),
        "---\ntitle: Role Demo\nslug: role-demo\nmilestone: \"0.9.0\"\nstatus: in_progress\n\
         description: >\n  Scratch fixture epic for the per-phase assignment demo.\n---\n\n\
         # Epic — Role Demo\n",
    )
    .expect("write epic README");

    std::fs::write(
        repo.join("docs/epics/role-demo/prd-role-demo.md"),
        "---\nprd: prd-role-demo\nepic: role-demo\nmilestone: \"0.9.0\"\nstatus: proposed\n\
         description: >\n  Two-loop fixture: a dev-role agent builds, a QA-role agent verifies.\n---\n\n\
         # PRD — Role Demo\n\n\
         ## Phases\n\n\
         ### Phase 1 — Dev build\n\
         - [ ] `role-demo/dev-build` Create `math.js` exporting `add(a, b)` (CommonJS), and \
         `math.test.js` using `node:assert` that asserts add(2,3)===5 and add(-1,1)===0; run \
         `node math.test.js` and make it pass\n\n\
         ### Phase 2 — QA verify\n\
         - [ ] `role-demo/qa-verify` Verify the dev work: re-run `node math.test.js`, review \
         `math.js` for correctness, then write `qa-report.md` whose final line is exactly \
         `**QA verdict:** PASS` (or BLOCK when broken)\n",
    )
    .expect("write fixture PRD");

    // A git repo so spawned agents' git habits don't error out.
    assert!(std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&repo)
        .status()
        .expect("git init")
        .success());

    repo
}

/// The demo's own per-phase prompt (see module docs for why not
/// `build_combined_phase_prompt`).
fn demo_prompt(loop_id: &str, title: &str, phase: &str) -> String {
    let scope = if phase == "Phase 1 — Dev build" {
        "This is a scratch fixture repo — do NOT create branches, commits, PRs, or run any \
         loopdeck skills; just write the files and run the check named in the loop."
    } else {
        "This is a scratch fixture repo — do NOT create branches, commits, PRs, or run any \
         loopdeck skills; do not modify the dev work unless it is broken (if you must fix it, \
         say so in the report)."
    };
    format!(
        "You are working on this scratch fixture repo as part of a two-phase role-demo run. \
         Run loop `{loop_id}` — \"{title}\". Implement it per the checklist item's own wording \
         in `docs/epics/role-demo/prd-role-demo.md`. {scope} Do not ask clarifying questions — \
         make reasonable assumptions. Your final message must be one short line summarizing \
         what you did."
    )
}

#[tokio::test]
#[ignore = "spawns real claude sessions and temp roster entries; run explicitly"]
async fn role_demo_two_phase_plan_dev_builds_qa_verifies() {
    let repo = fixture_repo();

    // ── Roster: temp dev/QA entries in the real registry, removed after. ──
    let disk_registry = GlobalConfig::load().ok();
    let mut registry = disk_registry.clone().unwrap_or_default();
    let mut dev =
        NamedAgentConfig::new("Dev Role (demo)".into(), AgentConfig::default()).expect("dev entry");
    dev.charter = demo_charter(
        "You are the dev-role agent. You build small, correct increments; you do not verify \
         or review beyond your own check.",
        "Keep every change minimal. Final message: one short line.",
    );
    let mut qa =
        NamedAgentConfig::new("QA Role (demo)".into(), AgentConfig::default()).expect("qa entry");
    qa.charter = demo_charter(
        "You are the QA-role agent. You verify work; you do not build.",
        "Every report ends with a literal `**QA verdict:** PASS` or `**QA verdict:** BLOCK` line.",
    );
    registry.agents.push(dev.clone());
    registry.agents.push(qa.clone());
    if disk_registry.is_some() {
        registry.save().expect("save registry with demo entries");
    }
    let _restore = RegistryRestore(disk_registry);

    let state = AppState {
        config: Mutex::new(registry),
        claude_sessions: Mutex::new(HashMap::new()),
        pending_answers: Mutex::new(HashMap::new()),
        pending_permissions: Mutex::new(HashMap::new()),
        pending_plans: Mutex::new(HashMap::new()),
        interrupt_slots: Mutex::new(HashMap::new()),
        run_handles: Mutex::new(HashMap::new()),
        multi_agent_active_runs: Mutex::new(HashSet::new()),
        multi_agent_manifest_locks: Mutex::new(HashMap::new()),
    };

    // ── Plan: two phases, dev then QA (authored chain), each assigned. ──
    let execution_ids = vec![
        "role-demo/dev-build".to_string(),
        "role-demo/qa-verify".to_string(),
    ];
    let mut plan = build_run_plan(
        "role-demo".into(),
        repo.clone(),
        chrono::Utc::now(),
        &execution_ids,
        StallPolicy::ContinueIndependent,
        false,
    );
    plan.phases[0].assigned_agent = Some(dev.id.clone());
    plan.phases[1].assigned_agent = Some(qa.id.clone());
    runplan::save(&repo, &plan).expect("save demo plan");
    let mut plan = runplan::load(&repo)
        .expect("reload plan")
        .expect("plan present");
    let batch_count_before = plan.phases.len();

    // ── Execute: the executor's own batching + resolution + spawn path. ──
    while let Some(batch) = next_queued_batch(&plan) {
        let assigned = plan.phases[batch[0]]
            .assigned_agent
            .clone()
            .expect("demo phases are all assigned");
        let agent_config =
            resolve_agent_config_by_id(&state, &assigned).expect("temp roster entry resolves");
        assert!(
            agent_config.charter.is_some(),
            "charter must ride the config"
        );

        let locs: Vec<(String, epic::LoopLocation)> = batch
            .iter()
            .map(|&idx| {
                let execution_id = plan.phases[idx].execution_id.clone();
                let loc =
                    epic::find_loop_by_id(&repo, &execution_id).expect("fixture loop resolves");
                (execution_id, loc)
            })
            .collect();
        // One agent per batch — the Phase 4 split guarantee under test.
        assert_eq!(batch.len(), 1, "each demo phase has its own agent");

        for &idx in &batch {
            plan.phases[idx].status = RunPhaseStatus::Running;
        }
        runplan::save(&repo, &plan).expect("persist running state");

        let (_, loc) = &locs[0];
        let prompt = demo_prompt(&locs[0].0, &loc.title, &loc.phase);
        let channel: Channel<ClaudeEvent> = Channel::new(|_| Ok(()));
        let token_budget = TokenBudget::new(300_000);
        let started = Instant::now();
        let response = start_fresh_and_record_streaming_in_root_with_config(
            &state,
            &repo,
            &repo,
            &prompt,
            Some(loc.title.clone()),
            &channel,
            Some(&token_budget),
            Some(&agent_config),
            None,
            true,
            false,
        )
        .await
        .expect("demo phase turn completes");

        let tokens = response
            .usage
            .as_ref()
            .map(|usage| usage.input_tokens.saturating_add(usage.output_tokens))
            .unwrap_or_default();
        for &idx in &batch {
            plan.phases[idx].status = RunPhaseStatus::Completed;
            plan.phases[idx].token_usage = tokens;
            plan.phases[idx].wall_clock_secs = started.elapsed().as_secs();
        }
        runplan::save(&repo, &plan).expect("persist completed phase");
    }
    assert_eq!(batch_count_before, 2);

    // ── Assert the work really happened: dev built, QA verified. ──
    let math = std::fs::read_to_string(repo.join("math.js")).expect("dev turn wrote math.js");
    assert!(math.contains("add"), "math.js exports add: {math}");
    assert!(
        repo.join("math.test.js").exists(),
        "dev turn wrote math.test.js"
    );
    let qa_report =
        std::fs::read_to_string(repo.join("qa-report.md")).expect("qa turn wrote qa-report.md");
    assert!(
        qa_report.trim_end().ends_with("**QA verdict:** PASS"),
        "qa-report must end with PASS verdict:\n{qa_report}"
    );

    // ── Report: per-role attribution. ──
    let report = crate::runplan::RunReport::from_plan(plan, crate::runplan::AuditSlice::default());
    assert_eq!(
        report.phases[0].assigned_agent.as_deref(),
        Some(dev.id.as_str())
    );
    assert_eq!(
        report.phases[1].assigned_agent.as_deref(),
        Some(qa.id.as_str())
    );
    assert_eq!(report.phases[0].verdict, crate::runplan::PhaseVerdict::Pass);
    assert_eq!(report.phases[1].verdict, crate::runplan::PhaseVerdict::Pass);

    let summary = format!(
        "# Role demo run report\n\n\
         | Phase | Loop | Agent | Verdict | Tokens | Wall\n\
         |---|---|---|---|---|---\n{}",
        report
            .phases
            .iter()
            .map(|p| format!(
                "| {} | `{}` | {} | {:?} | {} | {}s |",
                p.execution_id,
                p.execution_id,
                match p.assigned_agent.as_deref() {
                    Some(id) if id == dev.id => "Dev Role (demo)",
                    Some(id) if id == qa.id => "QA Role (demo)",
                    _ => "default",
                },
                p.verdict,
                p.token_usage,
                p.wall_clock_secs,
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
    std::fs::write(repo.join(".loopdeck/role-demo-report.md"), &summary)
        .expect("write demo report");
    println!("\n{summary}\n\nfixture repo: {}", repo.display());
}
