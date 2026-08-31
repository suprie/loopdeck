//! Verified-delivery reconciliation — `prd-verified-delivery-reconciliation.md`.
//!
//! Data model + pure evaluator for the delivery links a loop accumulates
//! (branch, PRD, rubric result, PR) and the mismatch states that arise when
//! those links disagree with live Git / checklist state. Persisted inside
//! `.loopdeck/execution.yaml` on the loop records ([`crate::execution`]);
//! this module never touches disk — the evaluator is pure so the UI report,
//! the delivery gates, and tests all share one definition of "conflicting
//! records" (`reliable-delivery-bookkeeping` Phase 1, loop
//! `delivery-bookkeeping/reconciliation-model`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Rubric result ───────────────────────────────────────────────────

/// One per-criterion row of a `loopdeck-prd-verifier` report. Statuses mirror
/// the skill's own vocabulary (PASS / PARTIAL / FAIL).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CriterionStatus {
    Pass,
    Partial,
    Fail,
}

impl CriterionStatus {
    fn from_verifier_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_uppercase().as_str() {
            "PASS" => Some(Self::Pass),
            "PARTIAL" => Some(Self::Partial),
            "FAIL" => Some(Self::Fail),
            _ => None,
        }
    }
}

/// One criterion's evaluation, retained verbatim enough to audit later.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CriterionResult {
    pub criterion: String,
    pub status: CriterionStatus,
    /// Evidence cell (`file:line — quote`) from the verifier table.
    #[serde(default)]
    pub note: String,
}

/// The retained PRD-rubric result for one delivery. The roll-up verdict plus
/// the per-criterion rows that produced it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RubricResult {
    pub verdict: RubricVerdict,
    pub checked_at: DateTime<Utc>,
    pub criteria: Vec<CriterionResult>,
}

/// Roll-up verdict, same scale the verifier skill reports.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RubricVerdict {
    Pass,
    Warn,
    Fail,
}

impl RubricResult {
    /// The delivery pass bar: **every** criterion passes (`PARTIAL` is
    /// non-passing — a known-missing edge case must not ship as verified).
    pub fn all_pass(&self) -> bool {
        self.verdict == RubricVerdict::Pass
            && self.criteria.iter().all(|c| c.status == CriterionStatus::Pass)
    }
}

// ── Delivery links ──────────────────────────────────────────────────

/// The persisted delivery links for one loop — everything the reconciliation
/// compares. Lives on `ActiveLoop` while in flight (branch + rubric can exist
/// pre-completion) and on `HistoryLoop` as the terminal delivery record
/// (branch + commit + PR + rubric after `commit-push-pr`). The PR provider is
/// optional because provider support beyond GitHub is an open PRD question;
/// `None` means "recorded without a provider claim".
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DeliveryLinks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rubric: Option<RubricResult>,
}

// ── Mismatch model + evaluator ──────────────────────────────────────

/// A disagreement between persisted delivery links and live state. Each kind
/// names the two records that conflict, per the PRD's "report the exact
/// conflicting records" requirement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MismatchKind {
    /// Recorded branch no longer exists locally.
    BranchMissing,
    /// Recorded commit is not reachable from the recorded branch.
    CommitDiverged,
    /// The worktree's live branch is not the branch recorded for the loop.
    WrongBranch,
    /// Loop completed with delivery links but no PR URL.
    PrLinkMissing,
    /// Delivered loop retained no rubric result.
    RubricMissing,
    /// Retained rubric did not pass every criterion.
    RubricNotPassing,
    /// Loop history says completed but the PRD checklist item is unchecked.
    ChecklistIncomplete,
    /// PRD checklist item is checked but no PR link was ever recorded.
    ChecklistPremature,
}

impl std::fmt::Display for MismatchKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::BranchMissing => "recorded branch is missing from the local repo",
            Self::CommitDiverged => "recorded commit is not reachable from the recorded branch",
            Self::WrongBranch => "live branch differs from the branch recorded for this loop",
            Self::PrLinkMissing => "delivery has no pull-request link",
            Self::RubricMissing => "no PRD rubric result was retained",
            Self::RubricNotPassing => "retained rubric result is not passing",
            Self::ChecklistIncomplete => "loop is completed but its PRD checklist item is unchecked",
            Self::ChecklistPremature => {
                "PRD checklist item is checked but no PR link was recorded"
            }
        };
        f.write_str(text)
    }
}

/// The live facts the persisted links are reconciled against, gathered by the
/// caller (IPC command or gate) so the evaluator itself stays pure and
/// side-effect free.
#[derive(Debug, Clone, Default)]
pub struct LiveDeliveryState {
    /// Branch currently checked out where the user stands.
    pub current_branch: Option<String>,
    /// Whether the recorded branch still exists locally.
    pub branch_exists: bool,
    /// Whether the recorded commit resolves and is reachable from the
    /// recorded branch's tip.
    pub commit_on_branch: bool,
    /// PRD checklist state for this loop's item (`None` = item not found).
    pub checklist_checked: Option<bool>,
}

/// Reconcile one loop's persisted delivery links against live state. Empty
/// result = verified, no discrepancy. A loop with no links at all yields no
/// mismatches (nothing has been claimed yet — absence is not drift).
pub fn reconcile_delivery(
    links: Option<&DeliveryLinks>,
    live: &LiveDeliveryState,
) -> Vec<MismatchKind> {
    let Some(links) = links else {
        return Vec::new();
    };
    let mut mismatches = Vec::new();

    if let Some(branch) = links.branch.as_deref().filter(|b| !b.is_empty()) {
        if !live.branch_exists {
            mismatches.push(MismatchKind::BranchMissing);
        } else if !live.commit_on_branch && links.commit.is_some() {
            mismatches.push(MismatchKind::CommitDiverged);
        }
        if let Some(current) = live.current_branch.as_deref() {
            if current != branch {
                mismatches.push(MismatchKind::WrongBranch);
            }
        }
    }

    match &links.rubric {
        None => mismatches.push(MismatchKind::RubricMissing),
        Some(rubric) if !rubric.all_pass() => mismatches.push(MismatchKind::RubricNotPassing),
        Some(_) => {}
    }

    if links.commit.is_some() && links.pr_url.is_none() {
        mismatches.push(MismatchKind::PrLinkMissing);
    }

    match live.checklist_checked {
        Some(true) if links.pr_url.is_none() => mismatches.push(MismatchKind::ChecklistPremature),
        Some(false) if links.pr_url.is_some() => mismatches.push(MismatchKind::ChecklistIncomplete),
        _ => {}
    }

    mismatches
}

// ── Delivery gates (Phase 3) ────────────────────────────────────────

/// Why a delivery was blocked before any completion mutation. Evaluated fresh
/// at delivery time — never from a stale persisted verdict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateBlock {
    /// The loop's checklist item is already complete (and no open PR claims
    /// it) — nothing left to deliver.
    LoopNotPending,
    /// The worktree is not on the branch recorded for the loop.
    BranchMismatch,
    /// The loop's PRD checklist item no longer resolves.
    PrdLinkMissing,
    /// No rubric result was produced for this delivery attempt.
    RubricMissing,
    /// The rubric ran and at least one criterion did not pass.
    RubricNotPassing,
}

impl std::fmt::Display for GateBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::LoopNotPending => "the loop's checklist item is already complete",
            Self::BranchMismatch => "current worktree branch is not the branch recorded for the loop",
            Self::PrdLinkMissing => "the loop is not linked to a PRD checklist item",
            Self::RubricMissing => "no PRD rubric result was produced for this delivery",
            Self::RubricNotPassing => "PRD rubric did not pass every criterion",
        };
        f.write_str(text)
    }
}

/// Evaluate the ordered delivery gates (PRD "Delivery gate" steps 1-4). An
/// empty result means delivery may proceed. Shared by the run executor (the
/// real gate) and the UI report (so the user sees the same verdict the
/// automation would enforce).
pub fn evaluate_delivery_gates(
    loop_pending: Option<bool>,
    branch_matches: bool,
    prd_linked: bool,
    rubric: Option<&RubricResult>,
) -> Vec<GateBlock> {
    let mut blocks = Vec::new();
    if loop_pending == Some(false) {
        blocks.push(GateBlock::LoopNotPending);
    }
    if !branch_matches {
        blocks.push(GateBlock::BranchMismatch);
    }
    if !prd_linked {
        blocks.push(GateBlock::PrdLinkMissing);
    }
    match rubric {
        None => blocks.push(GateBlock::RubricMissing),
        Some(result) if !result.all_pass() => blocks.push(GateBlock::RubricNotPassing),
        Some(_) => {}
    }
    blocks
}

// ── Rubric extraction ───────────────────────────────────────────────

/// Parse the `loopdeck-prd-verifier` report out of a turn's final response:
/// the per-criterion table rows (`| n | criterion | STATUS | evidence |`)
/// plus the `**Verdict:**` roll-up. Uses the **last** report in the text,
/// matching `run_executor::extract_verdict`'s convention for multi-loop turns.
/// Returns `None` when neither a table row nor a verdict line is present.
pub fn extract_rubric_result(text: &str, checked_at: DateTime<Utc>) -> Option<RubricResult> {
    let mut criteria = Vec::new();
    let mut verdict = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("**Verdict:**") {
            verdict = match rest.trim() {
                "PASS" => Some(RubricVerdict::Pass),
                "WARN" => Some(RubricVerdict::Warn),
                "BLOCK" => Some(RubricVerdict::Fail),
                _ => None,
            };
            continue;
        }
        if !trimmed.starts_with('|') {
            continue;
        }
        // Table row: | # | criterion | status | evidence | — header and
        // separator rows fail the column/status parse and are skipped.
        let cells: Vec<String> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect();
        if cells.len() < 3 {
            continue;
        }
        let Some(status) = CriterionStatus::from_verifier_token(&cells[2]) else {
            continue;
        };
        criteria.push(CriterionResult {
            criterion: cells[1].clone(),
            status,
            note: cells.get(3).cloned().unwrap_or_default(),
        });
    }
    if criteria.is_empty() && verdict.is_none() {
        return None;
    }
    Some(RubricResult {
        verdict: verdict.unwrap_or(RubricVerdict::Fail),
        checked_at,
        criteria,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000, 0).unwrap()
    }

    fn links() -> DeliveryLinks {
        DeliveryLinks {
            branch: Some("run/x".into()),
            commit: Some("abc123".into()),
            pr_url: Some("https://github.com/o/r/pull/1".into()),
            pr_provider: Some("github".into()),
            rubric: Some(RubricResult {
                verdict: RubricVerdict::Pass,
                checked_at: ts(),
                criteria: vec![CriterionResult {
                    criterion: "model persisted".into(),
                    status: CriterionStatus::Pass,
                    note: "src-tauri/src/delivery.rs:1".into(),
                }],
            }),
        }
    }

    fn live() -> LiveDeliveryState {
        LiveDeliveryState {
            current_branch: Some("run/x".into()),
            branch_exists: true,
            commit_on_branch: true,
            checklist_checked: Some(true),
        }
    }

    #[test]
    fn reconcile_passes_a_consistent_delivery() {
        assert!(reconcile_delivery(Some(&links()), &live()).is_empty());
    }

    #[test]
    fn reconcile_absent_links_are_not_drift() {
        assert!(reconcile_delivery(None, &live()).is_empty());
    }

    #[test]
    fn reconcile_flags_every_disagreement() {
        let drifted = LiveDeliveryState {
            current_branch: Some("other".into()),
            branch_exists: false,
            commit_on_branch: false,
            checklist_checked: Some(false),
        };
        let found = reconcile_delivery(Some(&links()), &drifted);
        assert!(found.contains(&MismatchKind::BranchMissing));
        assert!(found.contains(&MismatchKind::WrongBranch));
        assert!(found.contains(&MismatchKind::ChecklistIncomplete));
    }

    #[test]
    fn reconcile_flags_missing_pr_premature_checklist_and_bad_rubric() {
        let mut l = links();
        l.pr_url = None;
        l.rubric = None;
        let found = reconcile_delivery(Some(&l), &live());
        assert!(found.contains(&MismatchKind::PrLinkMissing));
        assert!(found.contains(&MismatchKind::RubricMissing));
        // Checked item with no PR is premature regardless of the other links.
        assert!(found.contains(&MismatchKind::ChecklistPremature));
    }

    #[test]
    fn reconcile_flags_checked_item_without_pr() {
        let mut l = links();
        l.pr_url = None;
        let found = reconcile_delivery(Some(&l), &live());
        assert!(found.contains(&MismatchKind::ChecklistPremature));
    }

    #[test]
    fn partial_criterion_is_not_passing() {
        let mut l = links();
        l.rubric.as_mut().unwrap().criteria[0].status = CriterionStatus::Partial;
        let found = reconcile_delivery(Some(&l), &live());
        assert!(found.contains(&MismatchKind::RubricNotPassing));
    }

    #[test]
    fn gates_open_only_when_everything_aligns() {
        let rubric = links().rubric.unwrap();
        assert!(evaluate_delivery_gates(Some(true), true, true, Some(&rubric)).is_empty());
    }

    #[test]
    fn gates_report_each_blocked_dimension() {
        let rubric = links().rubric.unwrap();
        let blocks = evaluate_delivery_gates(Some(false), false, false, None);
        assert!(blocks.contains(&GateBlock::LoopNotPending));
        assert!(blocks.contains(&GateBlock::BranchMismatch));
        assert!(blocks.contains(&GateBlock::PrdLinkMissing));
        assert!(blocks.contains(&GateBlock::RubricMissing));
    }

    #[test]
    fn gates_block_on_failing_criterion() {
        let mut rubric = links().rubric.unwrap();
        rubric.criteria[0].status = CriterionStatus::Fail;
        assert_eq!(
            evaluate_delivery_gates(Some(true), true, true, Some(&rubric)),
            vec![GateBlock::RubricNotPassing]
        );
    }

    #[test]
    fn rubric_extractor_parses_verifier_report() {
        let text = "## PRD Verification — prd-x\n\n\
                    **Verdict:** WARN\n\n\
                    | # | Criterion | Status | Evidence |\n\
                    |---|-----------|--------|----------|\n\
                    | 1 | Define links | PASS | `delivery.rs:40` — struct |\n\
                    | 2 | Report UI | PARTIAL | `DeliveryReportTab.tsx:10` — happy path |\n";
        let rubric = extract_rubric_result(text, ts()).expect("parses");
        assert_eq!(rubric.verdict, RubricVerdict::Warn);
        assert_eq!(rubric.criteria.len(), 2);
        assert_eq!(rubric.criteria[1].status, CriterionStatus::Partial);
        assert!(!rubric.all_pass());
    }

    #[test]
    fn rubric_extractor_uses_last_verdict_and_skips_non_rows() {
        let text = "**Verdict:** BLOCK\nno table at all";
        let rubric = extract_rubric_result(text, ts()).expect("verdict alone parses");
        assert_eq!(rubric.verdict, RubricVerdict::Fail);
        assert!(rubric.criteria.is_empty());
    }

    #[test]
    fn rubric_extractor_none_when_no_signal() {
        assert!(extract_rubric_result("plain text", ts()).is_none());
    }
}
