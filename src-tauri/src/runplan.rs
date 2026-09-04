//! Persisted run-queue plan — `.loopdeck/run-plan.yaml`.
//!
//! Phase 1 of `prd-run-queue.md` (milestone 0.4.0, epic `overnight-orchestration`).
//! Captures *what* an unattended run executes, *with which pre-flight
//! answers*, and *under which consent and budgets* — the sequential executor
//! (Phase 2) turns this into one orchestrated `claude_session` per phase.
//!
//! This module is the **data layer only**: types, serde defaults, and atomic
//! persistence. No executor, no IPC commands, and no enforcement of the
//! budgets it records — budget enforcement belongs to `prd-unattended-ship`,
//! per this PRD's Non-Goals.
//!
//! Unlike [`crate::execution`], there is no optimistic-concurrency revision
//! or `.bak` recovery here: a run plan has exactly one writer (the in-app
//! executor), not the mix of CLI + UI callers `execution.yaml` supports.

use crate::error::AppError;
use crate::limits;
use crate::persist;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn default_status() -> RunPhaseStatus {
    RunPhaseStatus::Queued
}

/// What happens to the rest of the queue when a phase parks mid-run (ADR-5).
/// Chosen once, at queue time.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StallPolicy {
    /// Skip ahead to queued phases with no dependency on the parked one;
    /// dependents park transitively. Default — one ambiguous phase doesn't
    /// stop phases that don't need its output.
    #[default]
    ContinueIndependent,
    /// A park halts every remaining phase, preserving strict sequence.
    Halt,
}

/// Where a queued phase is in the run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RunPhaseStatus {
    #[default]
    Queued,
    Running,
    Parked,
    Delivered,
    Completed,
    Failed,
    Interrupted,
    Killed,
}

/// A pre-flight clarifying question and its answer, pinned into the plan
/// before the run starts (Phase 3) and injected into the phase's session
/// prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedAnswer {
    pub question: String,
    pub answer: String,
}

/// Whether a queued phase's pre-flight interview has been resolved.
/// `queue_run` refuses to start while any queued phase is still `Pending`
/// (PRD Phase 3: "block run start until every queued phase's interview is
/// answered or explicitly skipped").
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum InterviewStatus {
    #[default]
    Pending,
    Answered,
    Skipped,
}

/// Queue-time consent for the whole run (ADR-1). Selasar never asks again
/// once the run starts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RunConsent {
    /// Authorizes `gh pr create --draft` with no interactive confirmation on
    /// a green verify verdict. Draft-only; a draft is never auto-readied.
    #[serde(default)]
    pub draft_pr_authorized: bool,
}

/// Hard budget caps for the run (ADR-4). All optional: `None` means "use
/// `prd-unattended-ship`'s named-constant default," not "unbounded" — this
/// PRD only records the values it was queued with; `prd-unattended-ship`
/// enforces them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RunBudgets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_phase_token_cap: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_phase_wall_clock_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_run_wall_clock_secs: Option<u64>,
}

/// Isolation and cleanup state for one unattended run. This lives in the
/// plan so a restart can retain a failed worktree for inspection instead of
/// accidentally creating a second branch for the same run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RunEnvironment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub worktree_kept: bool,
}

/// One queued phase, joined to the spec/execution layer by its stable
/// execution ID ([`crate::epic::PrdLoop::id`], the same ID `execution.rs`
/// tracks) — never a free-text phase name, so a PRD rename after queuing
/// can't silently detach the run from its phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RunPhase {
    pub execution_id: String,
    #[serde(default = "default_status")]
    pub status: RunPhaseStatus,
    #[serde(default)]
    pub interview: Vec<PinnedAnswer>,
    /// Set by `run_phase_interview`/`skip_phase_interview` (Phase 3). Starts
    /// `Pending` for every newly queued phase.
    #[serde(default)]
    pub interview_status: InterviewStatus,
    /// Other phases in this plan (by `execution_id`) that must complete
    /// before this one is eligible under `continue_independent`. Defaults to
    /// the authored order — each phase depends on its predecessor — unless
    /// the picker UI (Phase 5) edits edges explicitly.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Roster id of the agent this phase is assigned to (`prd-role-foundations`
    /// Phase 4). `None` = the default agent config. Validated against the
    /// roster at `create_run_plan` time; the executor parks the phase if the
    /// entry was removed by the time the run executes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_agent: Option<String>,
    /// The reason shown in the morning report when a phase doesn't complete.
    /// Set when `status == Parked` (Phase 4 fills this with the actual
    /// question/permission-card payload) **or** `status == Failed` (Phase 2's
    /// executor sets it to the non-green verdict or turn error — Phase 2 has
    /// no interactive-stall handling yet, so a phase that doesn't advance is
    /// always a hard failure, never a park).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub park_payload: Option<String>,
    /// Terminal usage retained for the morning report and budget audit.
    #[serde(default)]
    pub token_usage: u64,
    /// Wall-clock time spent in the phase turn, rounded down to seconds.
    #[serde(default)]
    pub wall_clock_secs: u64,
}

/// The full run plan — the on-disk shape of `.loopdeck/run-plan.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunPlan {
    pub id: String,
    pub project: PathBuf,
    pub created: DateTime<Utc>,
    #[serde(default)]
    pub consent: RunConsent,
    #[serde(default)]
    pub budgets: RunBudgets,
    #[serde(default)]
    pub environment: RunEnvironment,
    /// Wall-clock time accumulated by the executor for the run report.
    #[serde(default)]
    pub wall_clock_secs: u64,
    #[serde(default)]
    pub stall_policy: StallPolicy,
    #[serde(default)]
    pub phases: Vec<RunPhase>,
}

/// Path to a project's run plan.
pub fn run_plan_path(repo_path: &Path) -> PathBuf {
    repo_path.join(".loopdeck").join("run-plan.yaml")
}

/// Load `.loopdeck/run-plan.yaml` for `repo_path`. `Ok(None)` means no plan
/// is queued (nothing on disk) — distinct from a malformed file, which
/// errors. See [`load_from_path`].
pub fn load(repo_path: &Path) -> Result<Option<RunPlan>, AppError> {
    load_from_path(&run_plan_path(repo_path))
}

/// Load from an explicit path. Missing file → `Ok(None)`; malformed file →
/// `Err` (the file is left untouched — this module never writes on load).
pub fn load_from_path(path: &Path) -> Result<Option<RunPlan>, AppError> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = limits::read_bounded_to_string(path, limits::RUN_PLAN_MAX_BYTES)?;
    serde_yaml::from_str::<RunPlan>(&contents)
        .map(Some)
        .map_err(|e| AppError::RunPlan(format!("run plan at {} is malformed: {e}", path.display())))
}

/// Persist `plan` to `.loopdeck/run-plan.yaml` for `repo_path`. See
/// [`save_to_path`].
pub fn save(repo_path: &Path, plan: &RunPlan) -> Result<(), AppError> {
    save_to_path(&run_plan_path(repo_path), plan)
}

/// Atomically write `plan` to `path` via [`persist::atomic_write`].
pub fn save_to_path(path: &Path, plan: &RunPlan) -> Result<(), AppError> {
    let yaml = serde_yaml::to_string(plan)?;
    persist::atomic_write(path, &yaml)?;
    Ok(())
}

// ── Morning report (prd-wake-up Phase 2) ────────────────────────────────────

/// Derived verdict label for one phase, extracted from its terminal state
/// without re-running the verifier. The morning report presents, never re-judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PhaseVerdict {
    Pass,
    Warn,
    Block,
    Killed,
    Failed,
    Parked,
    Running,
}

/// One row in the morning per-phase verdict table. Derived from the RunPhase
/// on-disk state; no new storage.
#[derive(Debug, Clone, Serialize)]
pub struct PhaseReportEntry {
    pub execution_id: String,
    pub status: RunPhaseStatus,
    pub verdict: PhaseVerdict,
    /// Roster id of the agent the phase was assigned to (`prd-role-foundations`
    /// Phase 4 per-role attribution); `None` = the default agent.
    pub assigned_agent: Option<String>,
    /// Extracted from `park_payload` for Completed phases that shipped a draft PR.
    pub draft_pr_url: Option<String>,
    /// The park/kill/fail reason, verbatim from `park_payload`.
    pub reason: Option<String>,
    pub token_usage: u64,
    pub wall_clock_secs: u64,
}

/// Audit summary for the overnight run window (prd-wake-up Phase 2 P1).
/// Summarized auto-allow count; floor denials itemized.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AuditSlice {
    /// How many tool calls were auto-allowed during the run window.
    pub auto_allow_count: u64,
    /// Floor denials, each as `"tool_name: input"`.
    pub floor_denials: Vec<String>,
}

/// The morning report read model — joins RunPlan with derived per-phase
/// verdicts and the overnight audit slice. No new storage.
#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub plan: RunPlan,
    pub phases: Vec<PhaseReportEntry>,
    pub audit: AuditSlice,
}

impl RunReport {
    /// Build the read model from the on-disk run plan. Verdict labels are
    /// derived from phase status + `park_payload` markers.
    pub fn from_plan(plan: RunPlan, audit: AuditSlice) -> Self {
        let phases = plan
            .phases
            .iter()
            .map(|p| {
                let (verdict, draft_pr_url) = derive_verdict(p);
                PhaseReportEntry {
                    execution_id: p.execution_id.clone(),
                    status: p.status,
                    verdict,
                    assigned_agent: p.assigned_agent.clone(),
                    draft_pr_url,
                    reason: p.park_payload.clone(),
                    token_usage: p.token_usage,
                    wall_clock_secs: p.wall_clock_secs,
                }
            })
            .collect();
        Self {
            plan,
            phases,
            audit,
        }
    }
}

/// Derive a human-readable verdict and optional PR URL from a RunPhase's
/// terminal state. Pure — only reads existing fields.
fn derive_verdict(p: &RunPhase) -> (PhaseVerdict, Option<String>) {
    match p.status {
        RunPhaseStatus::Completed => {
            let url = p
                .park_payload
                .as_deref()
                .and_then(|payload| payload.strip_prefix("draft PR: "))
                .map(|s| s.to_string());
            (PhaseVerdict::Pass, url)
        }
        RunPhaseStatus::Delivered => {
            let url = p
                .park_payload
                .as_deref()
                .and_then(|payload| payload.strip_prefix("draft PR: "))
                .and_then(|payload| payload.split('\n').next())
                .map(str::to_string);
            (PhaseVerdict::Pass, url)
        }
        RunPhaseStatus::Parked => {
            let payload = p.park_payload.as_deref().unwrap_or_default();
            if payload.contains("verdict: BLOCK") {
                (PhaseVerdict::Block, None)
            } else if payload.contains("verdict: WARN") {
                (PhaseVerdict::Warn, None)
            } else {
                (PhaseVerdict::Parked, None)
            }
        }
        RunPhaseStatus::Killed => (PhaseVerdict::Killed, None),
        RunPhaseStatus::Failed => (PhaseVerdict::Failed, None),
        RunPhaseStatus::Running => (PhaseVerdict::Running, None),
        RunPhaseStatus::Queued => (PhaseVerdict::Parked, None), // never reached, but derive
        RunPhaseStatus::Interrupted => (PhaseVerdict::Failed, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_plan() -> RunPlan {
        RunPlan {
            id: "run-1".to_string(),
            project: PathBuf::from("/repo"),
            created: Utc.with_ymd_and_hms(2026, 7, 28, 9, 0, 0).unwrap(),
            consent: RunConsent {
                draft_pr_authorized: true,
            },
            budgets: RunBudgets {
                per_phase_token_cap: Some(500_000),
                per_phase_wall_clock_secs: Some(3600),
                total_run_wall_clock_secs: None,
            },
            environment: RunEnvironment::default(),
            wall_clock_secs: 0,
            stall_policy: StallPolicy::Halt,
            phases: vec![RunPhase {
                execution_id: "prd-run-queue/phase-1".to_string(),
                status: RunPhaseStatus::Queued,
                interview: vec![PinnedAnswer {
                    question: "Which stall policy?".to_string(),
                    answer: "halt".to_string(),
                }],
                interview_status: InterviewStatus::Answered,
                depends_on: vec![],
                assigned_agent: None,
                park_payload: None,
                token_usage: 0,
                wall_clock_secs: 0,
            }],
        }
    }

    #[test]
    fn serde_round_trip() {
        let plan = sample_plan();
        let yaml = serde_yaml::to_string(&plan).unwrap();
        let back: RunPlan = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(plan, back);
    }

    #[test]
    fn missing_optional_fields_default() {
        let yaml = r#"
id: run-2
project: /repo
created: 2026-07-28T09:00:00Z
phases:
  - execution_id: prd-run-queue/phase-1
"#;
        let plan: RunPlan = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(plan.consent, RunConsent::default());
        assert_eq!(plan.budgets, RunBudgets::default());
        assert_eq!(plan.stall_policy, StallPolicy::ContinueIndependent);
        assert_eq!(plan.phases.len(), 1);
        let phase = &plan.phases[0];
        assert_eq!(phase.status, RunPhaseStatus::Queued);
        assert!(phase.interview.is_empty());
        assert_eq!(phase.interview_status, InterviewStatus::Pending);
        assert!(phase.depends_on.is_empty());
        assert_eq!(phase.park_payload, None);
    }

    #[test]
    fn malformed_file_errors_without_touching_disk() {
        let dir = std::env::temp_dir().join(format!("loopdeck-runplan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run-plan.yaml");
        std::fs::write(&path, "id: [this is not a valid RunPlan").unwrap();

        let err = load_from_path(&path).unwrap_err();
        assert!(matches!(err, AppError::RunPlan(_)));
        // The malformed file is left exactly as written — load never rewrites.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "id: [this is not a valid RunPlan"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_loads_as_none() {
        let dir = std::env::temp_dir().join(format!("loopdeck-runplan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir).unwrap(), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn save_then_load_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("loopdeck-runplan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let plan = sample_plan();

        save(&dir, &plan).unwrap();
        let loaded = load(&dir).unwrap().unwrap();
        assert_eq!(loaded, plan);

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── Morning report fixture tests (prd-wake-up Phase 3) ──────────────────

    fn fixture_plan() -> RunPlan {
        RunPlan {
            id: "test-run-1".into(),
            project: PathBuf::from("/repo"),
            created: chrono::Utc::now(),
            consent: RunConsent {
                draft_pr_authorized: true,
            },
            budgets: RunBudgets::default(),
            environment: RunEnvironment::default(),
            wall_clock_secs: 3600,
            stall_policy: StallPolicy::ContinueIndependent,
            phases: vec![
                // Completed phase with draft PR
                RunPhase {
                    execution_id: "epic/prd/phase-completed".into(),
                    status: RunPhaseStatus::Completed,
                    park_payload: Some("draft PR: https://github.com/user/repo/pull/42".into()),
                    token_usage: 125000,
                    wall_clock_secs: 600,
                    ..Default::default()
                },
                // Parked phase with WARN verdict
                RunPhase {
                    execution_id: "epic/prd/phase-warn".into(),
                    status: RunPhaseStatus::Parked,
                    park_payload: Some("verify verdict: WARN".into()),
                    token_usage: 89000,
                    wall_clock_secs: 420,
                    ..Default::default()
                },
                // Parked phase with BLOCK verdict
                RunPhase {
                    execution_id: "epic/prd/phase-block".into(),
                    status: RunPhaseStatus::Parked,
                    park_payload: Some("verify verdict: BLOCK".into()),
                    token_usage: 67000,
                    wall_clock_secs: 310,
                    ..Default::default()
                },
                // Killed phase (token budget)
                RunPhase {
                    execution_id: "epic/prd/phase-killed".into(),
                    status: RunPhaseStatus::Killed,
                    park_payload: Some("phase token budget exceeded".into()),
                    token_usage: 500000,
                    wall_clock_secs: 1800,
                    ..Default::default()
                },
                // Failed phase
                RunPhase {
                    execution_id: "epic/prd/phase-failed".into(),
                    status: RunPhaseStatus::Failed,
                    park_payload: Some(
                        "no verify verdict found in the turn's final response".into(),
                    ),
                    token_usage: 200000,
                    wall_clock_secs: 900,
                    ..Default::default()
                },
            ],
        }
    }

    #[test]
    fn report_derives_correct_verdicts() {
        let plan = fixture_plan();
        let report = RunReport::from_plan(plan, AuditSlice::default());

        assert_eq!(report.phases.len(), 5);

        // Completed → Pass with PR URL
        assert_eq!(report.phases[0].verdict, PhaseVerdict::Pass);
        assert_eq!(
            report.phases[0].draft_pr_url,
            Some("https://github.com/user/repo/pull/42".into())
        );

        // Parked with WARN → Warn
        assert_eq!(report.phases[1].verdict, PhaseVerdict::Warn);
        assert!(report.phases[1].draft_pr_url.is_none());

        // Parked with BLOCK → Block
        assert_eq!(report.phases[2].verdict, PhaseVerdict::Block);

        // Killed → Killed
        assert_eq!(report.phases[3].verdict, PhaseVerdict::Killed);
        assert!(report.phases[3]
            .reason
            .as_deref()
            .unwrap()
            .contains("token budget"));

        // Failed → Failed
        assert_eq!(report.phases[4].verdict, PhaseVerdict::Failed);
    }

    #[test]
    fn report_keeps_delivered_pr_as_pass_with_its_url() {
        let mut plan = sample_plan();
        plan.phases[0].status = RunPhaseStatus::Delivered;
        plan.phases[0].park_payload = Some(
            "draft PR: https://github.com/acme/repo/pull/42\ndelivered — PRD link needs repair"
                .into(),
        );

        let report = RunReport::from_plan(plan, AuditSlice::default());
        assert_eq!(report.phases[0].verdict, PhaseVerdict::Pass);
        assert_eq!(
            report.phases[0].draft_pr_url.as_deref(),
            Some("https://github.com/acme/repo/pull/42")
        );
    }

    #[test]
    fn report_preserves_token_and_wall_clock() {
        let plan = fixture_plan();
        let report = RunReport::from_plan(plan, AuditSlice::default());

        assert_eq!(report.phases[0].token_usage, 125000);
        assert_eq!(report.phases[0].wall_clock_secs, 600);
        assert_eq!(report.phases[3].token_usage, 500000);
        assert_eq!(report.phases[3].wall_clock_secs, 1800);
    }

    #[test]
    fn report_derives_parked_verdict_for_generic_parked_phase() {
        let phase = RunPhase {
            execution_id: "x".into(),
            status: RunPhaseStatus::Parked,
            park_payload: Some("turn deadline elapsed".into()),
            ..Default::default()
        };
        let (verdict, url) = derive_verdict(&phase);
        assert_eq!(verdict, PhaseVerdict::Parked);
        assert!(url.is_none());
    }

    #[test]
    fn report_pass_without_pr_url_when_consent_missing() {
        // Completed with no draft PR in payload → should still be Pass but no URL
        let phase = RunPhase {
            execution_id: "x".into(),
            status: RunPhaseStatus::Completed,
            park_payload: Some(
                "green verification passed, but queue-time consent for an unattended draft PR is required"
                    .into(),
            ),
            ..Default::default()
        };
        let (verdict, url) = derive_verdict(&phase);
        assert_eq!(verdict, PhaseVerdict::Pass);
        assert!(url.is_none());
    }
}
