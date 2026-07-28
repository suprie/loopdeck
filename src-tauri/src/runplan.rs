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

/// Queue-time consent for the whole run (ADR-1). LoopDeck never asks again
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

/// One queued phase, joined to the spec/execution layer by its stable
/// execution ID ([`crate::epic::PrdLoop::id`], the same ID `execution.rs`
/// tracks) — never a free-text phase name, so a PRD rename after queuing
/// can't silently detach the run from its phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunPhase {
    pub execution_id: String,
    #[serde(default = "default_status")]
    pub status: RunPhaseStatus,
    #[serde(default)]
    pub interview: Vec<PinnedAnswer>,
    /// Other phases in this plan (by `execution_id`) that must complete
    /// before this one is eligible under `continue_independent`. Defaults to
    /// the authored order — each phase depends on its predecessor — unless
    /// the picker UI (Phase 5) edits edges explicitly.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Set when `status == Parked`: the reason shown in the morning report.
    /// Phase 4 (stall-policy runtime) replaces this with the actual
    /// question / permission-card payload; this placeholder only needs to
    /// round-trip today.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub park_payload: Option<String>,
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
            stall_policy: StallPolicy::Halt,
            phases: vec![RunPhase {
                execution_id: "prd-run-queue/phase-1".to_string(),
                status: RunPhaseStatus::Queued,
                interview: vec![PinnedAnswer {
                    question: "Which stall policy?".to_string(),
                    answer: "halt".to_string(),
                }],
                depends_on: vec![],
                park_payload: None,
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
}
