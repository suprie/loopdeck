# Decisions

_Older decisions archived to [decisions-archive.md](./decisions-archive.md)._

## 2026-08-31 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on Selasar development.

## 2026-08-31 — PR-backed runs complete the PRD checklist idempotently
- **Status**: accepted
- **Context**: A successful unattended PR completed runtime state without marking its spec checklist item, leaving delivery state to drift.
- **Consequences**: New runs live under `.loopdeck/runs/<branch-name>/`; after recording a draft PR, the runner checks the matching PRD item without ever reopening one on retry.

## 2026-08-31 — Pre-flight interviews can be answered as one contextual batch
- **Status**: accepted
- **Context**: A multi-phase night plan required a separate tap and agent turn for every pending interview, fragmenting the shared work context.
- **Consequences**: The wizard now offers one combined interview that presents all questions together and pins the phase-tagged answers back to their respective loops.

## 2026-08-31 — Delivery links live on the execution.yaml loop records; reconciliation is a pure evaluator
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 1. Branch/commit/PR/rubric links had nowhere persisted to live, and "conflicting records" was defined nowhere — the UI report, the delivery gates, and tests each risked their own drift definition.
- **Consequences**: New `src-tauri/src/delivery.rs`: `DeliveryLinks` (branch, commit, pr_url, optional pr_provider, optional `RubricResult`) persisted as an optional field on both `ActiveLoop` and `HistoryLoop` in `execution.yaml` (copied through `complete_current`/`abandon_current`), plus a pure `reconcile_delivery(links, LiveDeliveryState) -> Vec<MismatchKind>` and `evaluate_delivery_gates(...) -> Vec<GateBlock>` shared by report, gates, and tests. `extract_rubric_result` parses a `loopdeck-prd-verifier` report (per-criterion rows + `**Verdict:**`, last occurrence wins) into the retained `RubricResult`; provider stays optional because non-GitHub support is an open PRD question.

## 2026-08-31 — Run worktrees consolidate under `.loopdeck/runs/` and resume recreates from the surviving branch
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 2. Run worktrees scattered across legacy locations (`.loopdeck-runs/`, `.loopdeck-agent-worktrees/`) with no single managed home; a deleted worktree directory killed an otherwise-healthy run even when its branch survived.
- **Consequences**: `run_queue.rs::ensure_worktree` and `multi_agent.rs` both place worktrees under `.loopdeck/runs/` (gitignored via idempotent `ignore_managed_runs_dir`). On resume, a missing worktree directory is recreated at the same path from the surviving branch (`git worktree add` with the existing branch checked out); only a missing branch (or no recorded branch) errors. External/legacy worktrees are detected and classified (`detect_external_worktrees`: managed / legacy-run / legacy-multi-agent / claude-harness / user-manual) for the delivery report — never moved or deleted.

## 2026-08-31 — Delivery gates run a fresh rubric and own the loop's terminal state
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 3. Checklist completion could happen without branch/PRD/rubric/PR evidence, and a rerun after a delivered PR risked double-delivering.
- **Consequences**: `execute_run`'s success path parses the turn's own verifier report into a `RubricResult` and evaluates `evaluate_delivery_gates` fresh (loop pending, branch match, PRD link, rubric all-pass); any block parks the batch before any completion mutation. `DeliveryLinks` (branch, head commit, PR URL, provider, rubric) are persisted onto the loop before `complete_current`, and the PRD checklist items are checked only after the draft PR exists (`epic::complete_prd_loop`, idempotent). An idempotent-finish path (all checklist items already checked + an open PR on the branch) bypasses the gates so a retry after a successful delivery completes instead of re-delivering.

## 2026-09-01 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on Selasar development.

## 2026-09-01 — Clean handoff is lazy: record at delivery, cut the next worktree from the default branch at next run start
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 4, loop `clean-handoff`. PRD Open Question #1 (eager vs lazy next-branch creation) and the base of the next branch were pre-answered for the unattended run: lazy, base on the default branch, keep the delivered worktree.
- **Consequences**: New `src-tauri/src/handoff.rs` (`HandoffRecord` → `.loopdeck/handoff.yaml`: delivered branch, PR URL, retained worktree, `next_base`), persisted best-effort by `run_queue.rs::record_handoff` right after PR creation — it never fails the delivery. `ensure_worktree` now cuts every new run branch from `git::default_branch` (main/master, fallback HEAD) instead of whatever the main worktree has checked out, so a stray or delivered checkout can't leak into the next run's base; `finalize_worktree` retains the delivered worktree for review (supersedes prune-on-full-success). The `.loopdeck/runs/<next-branch>/` worktree itself is created only when the next run starts — no idle trees.

## 2026-09-01 — Failed deliveries persist a stage record; one idempotent retry resumes, nothing auto-retries
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 4, loop `retry-recovery`. Pre-answered clarification: no automatic retries; record how far the delivery got and expose one idempotent retry command (PRD P0 "offers the idempotent next action").
- **Consequences**: New `src-tauri/src/delivery_retry.rs`: `DeliveryStage` (nothing_mutated / committed / pushed) detected live from Git (`detect_stage`: dirty tree → nothing; clean + HEAD on a remote ref → pushed), `DeliveryRetryRecord` → `.loopdeck/delivery-retry.yaml` written by the executor at the failure site (with the retained rubric), and `run_retry` — re-detects the live stage, then requeue (nothing mutated) / push → adopt-or-create draft PR → finish bookkeeping (checklist → plan phases → `record_recovered_delivery` flips Abandoned → Completed with links) → clears the record. `gh` runs via PATH lookup so tests stub it with a script dir; push/PR failure keeps the record recoverable at the live stage. UI: `retry_delivery` command + `DeliveryReportTab` RetryCard (reason + `next_action`) and HandoffBanner.

## 2026-09-01 — Handoff spike validates file-based consumption (GO)
- **Status**: accepted
- **Context**: prd-handoff-spike needed evidence that a prompt-text-only consumer session reliably reads, respects, and cites an upstream file artifact.
- **Consequences**: Contract adopted at docs/epics/role-based-orchestration/handoff-artifact-contract.md; the spike run cited 17/17 artifact parts with no drift, truncation, or ignored input — GO recorded in prd-agent-handoff's Design section.
