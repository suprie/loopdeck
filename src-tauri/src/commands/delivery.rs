//! Delivery-reconciliation IPC — `prd-verified-delivery-reconciliation.md`
//! Phase 1 (verification / discrepancy report, fresh rubric run) and Phase 2
//! (external worktree detection). Read-only with one exception:
//! `run_delivery_rubric` persists the rubric result it just produced onto the
//! active loop's delivery links — it never completes anything.

use super::agent::start_fresh_and_record_streaming;
use super::state::{resolve_root, AppState};
use crate::agents::ClaudeEvent;
use crate::delivery::{
    self, evaluate_delivery_gates, extract_rubric_result, DeliveryLinks, GateBlock,
    LiveDeliveryState, MismatchKind, RubricResult,
};
use crate::epic;
use crate::error::AppError;
use crate::execution;
use crate::git::{self, WorktreeEntry};
use chrono::Utc;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;
use tauri::State;

/// One loop's reconciliation slice: its persisted links, the mismatches those
/// links create against live state, and the delivery-gate verdict the
/// automation would enforce right now.
#[derive(Serialize)]
pub struct LoopDeliveryReport {
    pub loop_id: String,
    pub title: String,
    pub in_progress: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<DeliveryLinks>,
    pub mismatches: Vec<MismatchKind>,
    pub gate_blocks: Vec<GateBlock>,
}

/// The full pre-mutation report — the "compare before any completion
/// mutation" surface. PR state inside it comes from the persisted link only;
/// the report never queries a provider live. Also carries the two Phase 4
/// surfaces: the latest clean-handoff record and any recoverable delivery
/// awaiting retry.
#[derive(Serialize)]
pub struct DeliveryReportResponse {
    pub loops: Vec<LoopDeliveryReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff: Option<crate::handoff::HandoffRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<RetryState>,
}

/// The recoverable-delivery slice the UI renders: the persisted record plus
/// the next safe action for its stage (PRD Phase 4 `retry-recovery`).
#[derive(Serialize)]
pub struct RetryState {
    pub record: crate::delivery_retry::DeliveryRetryRecord,
    pub next_action: &'static str,
}

/// Gather live Git + checklist facts for one loop's links, then reconcile.
fn report_for_loop(
    root: &Path,
    loop_id: &str,
    title: &str,
    in_progress: bool,
    links: Option<&DeliveryLinks>,
    current_branch: &Option<String>,
) -> LoopDeliveryReport {
    let branch = links
        .and_then(|l| l.branch.clone())
        .filter(|b| !b.is_empty());
    let live = LiveDeliveryState {
        current_branch: current_branch.clone(),
        branch_exists: branch
            .as_deref()
            .map(|b| git::branch_exists(root, b))
            .unwrap_or(false),
        commit_on_branch: match (&branch, links.and_then(|l| l.commit.clone())) {
            (Some(b), Some(c)) => git::commit_on_branch(root, b, &c),
            _ => false,
        },
        checklist_checked: epic::loop_checked(root, loop_id),
    };
    let mismatches = delivery::reconcile_delivery(links, &live);
    // Gate view for the *next* delivery attempt: the loop is pending when its
    // checklist item exists and is unchecked; the branch matches when nothing
    // recorded disagrees with where the user stands.
    let branch_matches = match (&branch, current_branch) {
        (Some(b), Some(c)) => c == b,
        _ => true,
    };
    let gate_blocks = evaluate_delivery_gates(
        live.checklist_checked.map(|checked| !checked),
        branch_matches,
        live.checklist_checked.is_some(),
        links.and_then(|l| l.rubric.as_ref()),
    );
    LoopDeliveryReport {
        loop_id: loop_id.to_string(),
        title: title.to_string(),
        in_progress,
        links: links.cloned(),
        mismatches,
        gate_blocks,
    }
}

/// Build the verification and discrepancy report: the active loop plus recent
/// history entries, each reconciled against live Git and checklist state.
#[tauri::command]
pub fn get_delivery_report(
    path: String,
    state: State<'_, AppState>,
) -> Result<DeliveryReportResponse, AppError> {
    let root = resolve_root(&state, &path)?;
    let loaded = execution::load(&root)?;
    let current_branch = git::current_branch(&root);

    let mut loops = Vec::new();
    if let Some(active) = &loaded.state.current {
        loops.push(report_for_loop(
            &root,
            &active.id,
            &active.title,
            true,
            active.delivery.as_ref(),
            &current_branch,
        ));
    }
    // Most recent deliveries first, bounded — the report explains *this*
    // delivery context, it is not an infinite audit log.
    for record in loaded.state.history.iter().rev().take(10) {
        if loaded
            .state
            .current
            .as_ref()
            .is_some_and(|active| active.id == record.id)
        {
            continue;
        }
        loops.push(report_for_loop(
            &root,
            &record.id,
            &record.title,
            false,
            record.delivery.as_ref(),
            &current_branch,
        ));
    }

    Ok(DeliveryReportResponse {
        loops,
        handoff: crate::handoff::load(&root)?,
        retry: crate::delivery_retry::load(&root)?.map(|record| RetryState {
            next_action: record.stage.next_action(),
            record,
        }),
    })
}

/// The one idempotent retry for a failed delivery (Phase 4 `retry-recovery`).
/// Resumes from the recorded stage — push the committed branch if needed,
/// adopt or create the draft PR, finish the bookkeeping — and reports what
/// it did. No automatic retries: only this user-initiated command.
#[tauri::command]
pub async fn retry_delivery(
    path: String,
    state: State<'_, AppState>,
) -> Result<crate::delivery_retry::RetryOutcome, AppError> {
    let root = resolve_root(&state, &path)?;
    crate::delivery_retry::run_retry(&root)
}

/// Prompt for a fresh, user-initiated PRD-rubric run on the active loop. The
/// agent verifier (`loopdeck-prd-verifier`) is the evaluator; the turn must
/// close with the skill's own report so `extract_rubric_result` can retain
/// the per-criterion rows.
fn build_rubric_prompt(epic_slug: &str, prd_slug: &str, phase: &str, title: &str) -> String {
    format!(
        "Run the `loopdeck-prd-verifier` skill against \
         `docs/epics/{epic_slug}/{prd_slug}.md` for the loop \"{title}\".\n\n\
         Scope: only the acceptance criteria of phase `{phase}` (this loop's \
         phase) plus the PRD's P0 goals those criteria serve — not the whole \
         PRD's phases. Read-only: do not edit any file.\n\n\
         End your final message with the verifier's report exactly as the \
         skill renders it — the per-criterion table and the \
         `**Verdict:** PASS | WARN | BLOCK` line — nothing after it."
    )
}

/// Run the PRD rubric fresh, right now, and retain the result on the active
/// loop's delivery links. This is the report's "verify before delivery"
/// action: an agent verifier turn in the main worktree whose parsed result is
/// persisted (branch link + rubric) so the discrepancy report and the
/// delivery gates stop reporting `rubric_missing`.
#[tauri::command]
pub async fn run_delivery_rubric(
    path: String,
    state: State<'_, AppState>,
) -> Result<RubricResult, AppError> {
    let root = resolve_root(&state, &path)?;
    let loaded = execution::load(&root)?;
    let active = loaded.state.current.clone().ok_or_else(|| {
        AppError::Conflict(
            "no loop is in progress; nothing to run a delivery rubric against".into(),
        )
    })?;
    let loc = epic::find_loop_by_id(&root, &active.id).ok_or_else(|| {
        AppError::ProjectNotFound(format!(
            "loop \"{}\" no longer resolves to a PRD checklist item",
            active.id
        ))
    })?;

    let prompt = build_rubric_prompt(&loc.epic, &loc.prd, &loc.phase, &active.title);
    // No-op sink channel: presence is what lets an AskUserQuestion park
    // instead of auto-denying (same trick as `run_phase_interview`).
    let channel: Channel<ClaudeEvent> = Channel::new(|_| Ok(()));
    let response = start_fresh_and_record_streaming(
        &state,
        &root,
        &prompt,
        Some(active.title.clone()),
        &channel,
    )
    .await?;
    let rubric = extract_rubric_result(&response.result, Utc::now()).ok_or_else(|| {
        AppError::RunPlan(
            "the verifier turn produced no rubric report (no criterion table, \
                 no verdict line) — nothing retained"
                .into(),
        )
    })?;

    // Persist onto the active loop's in-flight links; reload first — the
    // (possibly long) turn may have touched the file.
    let reloaded = execution::load(&root)?;
    let revision = reloaded.state.revision;
    let mut next = reloaded.state;
    let branch = next
        .current
        .as_ref()
        .and_then(|c| c.delivery.as_ref())
        .and_then(|d| d.branch.clone())
        .or_else(|| git::current_branch(&root));
    let links = next
        .current
        .as_ref()
        .and_then(|c| c.delivery.clone())
        .unwrap_or_default();
    let mut links = links;
    links.branch = branch;
    links.rubric = Some(rubric.clone());
    if let Some(current) = next.current.as_mut() {
        current.delivery = Some(links);
    }
    execution::save(&root, &next, revision)?;
    Ok(rubric)
}

// ── External worktree detection (Phase 2) ───────────────────────────

/// How a linked worktree relates to LoopDeck's managed locations. Everything
/// except `Managed` is "external": discovered and reported, never moved or
/// deleted automatically.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeClass {
    /// Under `<project>/.loopdeck/runs/` — where new managed worktrees live.
    Managed,
    /// A pre-containment run worktree (e.g. `../.loopdeck-runs/run-*`), on a
    /// `run/*` branch.
    LegacyRun,
    /// A multi-agent worktree from the legacy
    /// `../.loopdeck-agent-worktrees/` location or a `loopdeck/multi/*`
    /// branch.
    LegacyMultiAgent,
    /// A harness spike worktree under `<project>/.claude/worktrees/` —
    /// possibly in active use.
    ClaudeHarness,
    /// Anything user-created we cannot attribute.
    UserManual,
}

impl WorktreeClass {
    fn label(self) -> &'static str {
        match self {
            Self::Managed => "managed (.loopdeck/runs)",
            Self::LegacyRun => "legacy run worktree",
            Self::LegacyMultiAgent => "legacy multi-agent worktree",
            Self::ClaudeHarness => "harness worktree (.claude/worktrees)",
            Self::UserManual => "user-managed worktree",
        }
    }
}

/// One detected external worktree. Detect-only: no action is offered here by
/// design (relocation/resume actions are a later, explicitly user-initiated
/// surface).
#[derive(Serialize)]
pub struct ExternalWorktree {
    pub path: PathBuf,
    /// Short branch name (`run/foo`), `None` when detached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub classification: WorktreeClass,
    pub label: &'static str,
}

/// Pure classification of one inventory entry against a canonical project
/// root. Testable without a real repo.
fn classify_worktree(root: &Path, entry: &WorktreeEntry) -> Option<WorktreeClass> {
    let path = &entry.path;
    if path == root {
        return None; // the main worktree — not "external", not reported
    }
    let branch = entry
        .branch
        .as_deref()
        .map(|b| b.strip_prefix("refs/heads/").unwrap_or(b));
    if path.starts_with(root.join(".loopdeck").join("runs")) {
        return Some(WorktreeClass::Managed);
    }
    if path.starts_with(root.join(".claude").join("worktrees")) {
        return Some(WorktreeClass::ClaudeHarness);
    }
    if path.starts_with(root.join(".loopdeck-agent-worktrees")) {
        return Some(WorktreeClass::LegacyMultiAgent);
    }
    if branch.is_some_and(|b| b.starts_with("loopdeck/multi/")) {
        return Some(WorktreeClass::LegacyMultiAgent);
    }
    if branch.is_some_and(|b| b.starts_with("run/"))
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("run-"))
        || path.starts_with(".loopdeck-runs")
    {
        return Some(WorktreeClass::LegacyRun);
    }
    Some(WorktreeClass::UserManual)
}

/// Detect every linked worktree outside the managed `.loopdeck/runs/`
/// directory, each with a classification. Nothing is moved, deleted, or
/// modified — the PRD's "retained, never implicitly relocated" guarantee.
#[tauri::command]
pub fn detect_external_worktrees(
    path: String,
    state: State<'_, AppState>,
) -> Result<Vec<ExternalWorktree>, AppError> {
    let root = resolve_root(&state, &path)?;
    // Git reports canonical paths (`/var` → `/private/var`); match that so
    // starts_with comparisons hold on macOS.
    let canonical = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let inventory = git::worktree_inventory(&root).map_err(AppError::RunPlan)?;
    let mut found = Vec::new();
    for entry in &inventory {
        let Some(classification) = classify_worktree(&canonical, entry) else {
            continue;
        };
        if classification == WorktreeClass::Managed {
            continue; // contained already — not an external legacy tree
        }
        found.push(ExternalWorktree {
            path: entry.path.clone(),
            branch: entry
                .branch
                .as_deref()
                .map(|b| b.strip_prefix("refs/heads/").unwrap_or(b).to_string()),
            classification,
            label: classification.label(),
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, branch: Option<&str>) -> WorktreeEntry {
        WorktreeEntry {
            path: PathBuf::from(path),
            branch: branch.map(|b| format!("refs/heads/{b}")),
            detached: branch.is_none(),
            bare: false,
        }
    }

    #[test]
    fn main_worktree_is_not_reported() {
        assert!(classify_worktree(Path::new("/repo"), &entry("/repo", Some("main"))).is_none());
    }

    #[test]
    fn managed_runs_dir_is_not_external() {
        assert_eq!(
            classify_worktree(
                Path::new("/repo"),
                &entry("/repo/.loopdeck/runs/run/x", Some("run/x"))
            ),
            Some(WorktreeClass::Managed)
        );
    }

    #[test]
    fn claude_harness_spike_is_classified() {
        assert_eq!(
            classify_worktree(
                Path::new("/repo"),
                &entry("/repo/.claude/worktrees/spike-1", None)
            ),
            Some(WorktreeClass::ClaudeHarness)
        );
    }

    #[test]
    fn legacy_run_branch_outside_repo_is_classified() {
        assert_eq!(
            classify_worktree(
                Path::new("/repo"),
                &entry("/other/.loopdeck-runs/run-abc", Some("run/abc-1"))
            ),
            Some(WorktreeClass::LegacyRun)
        );
    }

    #[test]
    fn multi_agent_legacy_locations_are_classified() {
        assert_eq!(
            classify_worktree(
                Path::new("/repo"),
                &entry(
                    "/other/.loopdeck-agent-worktrees/r1/a1",
                    Some("loopdeck/multi/r1/x")
                )
            ),
            Some(WorktreeClass::LegacyMultiAgent)
        );
        // New containment location hosts them under .loopdeck/runs → Managed.
        assert_eq!(
            classify_worktree(
                Path::new("/repo"),
                &entry(
                    "/repo/.loopdeck/runs/multi/r1/a1",
                    Some("loopdeck/multi/r1/x")
                )
            ),
            Some(WorktreeClass::Managed)
        );
    }

    #[test]
    fn unknown_tree_is_user_manual() {
        assert_eq!(
            classify_worktree(
                Path::new("/repo"),
                &entry("/tmp/loopdeck-pr93", Some("codex/fix"))
            ),
            Some(WorktreeClass::UserManual)
        );
    }

    /// Real-git version of the classification tests above: actual
    /// `git worktree add` commands, one legacy run tree at the pre-containment
    /// `../.loopdeck-runs/` location and one managed under `.loopdeck/runs/`,
    /// both classified through the same inventory the command reads.
    #[test]
    fn real_git_worktrees_classify_by_location() {
        let dir =
            std::env::temp_dir().join(format!("loopdeck-delivery-wt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let run_git = |args: &[&str], cwd: &Path| {
            std::process::Command::new("git")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args(["-C"])
                .arg(cwd)
                .args(args)
                .output()
                .unwrap();
        };
        run_git(&["init", "-b", "main"], &dir);
        run_git(&["config", "user.email", "test@loopdeck.dev"], &dir);
        run_git(&["config", "user.name", "LoopDeck Test"], &dir);
        std::fs::write(dir.join("README.md"), "# test\n").unwrap();
        run_git(&["add", "."], &dir);
        run_git(&["commit", "-m", "initial"], &dir);

        let legacy = dir
            .parent()
            .unwrap()
            .join(".loopdeck-runs")
            .join(format!("run-{}", uuid::Uuid::new_v4()));
        let managed_branch = format!("run/{}", uuid::Uuid::new_v4());
        let managed = dir.join(".loopdeck").join("runs").join("run-managed");
        run_git(
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "run/legacy-abc",
                legacy.to_str().unwrap(),
            ],
            &dir,
        );
        run_git(
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                &managed_branch,
                managed.to_str().unwrap(),
            ],
            &dir,
        );

        let canonical = std::fs::canonicalize(&dir).unwrap();
        let inventory = git::worktree_inventory(&dir).unwrap();
        let legacy_class = inventory
            .iter()
            .find(|e| e.path == legacy || e.path == std::fs::canonicalize(&legacy).unwrap())
            .and_then(|e| classify_worktree(&canonical, e));
        let managed_class = inventory
            .iter()
            .find(|e| e.path == managed || e.path == std::fs::canonicalize(&managed).unwrap())
            .and_then(|e| classify_worktree(&canonical, e));

        assert_eq!(legacy_class, Some(WorktreeClass::LegacyRun));
        assert_eq!(managed_class, Some(WorktreeClass::Managed));

        run_git(
            &["worktree", "remove", "--force", legacy.to_str().unwrap()],
            &dir,
        );
        run_git(
            &["worktree", "remove", "--force", managed.to_str().unwrap()],
            &dir,
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(legacy.parent().unwrap()).ok();
    }
}
