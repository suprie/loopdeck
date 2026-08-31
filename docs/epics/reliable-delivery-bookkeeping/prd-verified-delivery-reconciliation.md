---
prd: prd-verified-delivery-reconciliation
epic: reliable-delivery-bookkeeping
milestone: "unassigned"
status: proposed
description: >
  Make the loop checklist the delivery-state single source of truth by
  reconciling the active branch, PRD rubric result, and pull-request result;
  automate verified commit, push, pull-request creation, completion, and a
  clean worktree handoff without ever merging a PR or rewriting Git history.
---

# PRD — Verified Delivery and Reconciliation

## Overview

LoopDeck should close the gap between an implementation branch and the
project's declared plan. A successful delivery run verifies the branch against
the active PRD, records the rubric in the delivery message, commits and pushes
the branch, opens a PR, marks the matching checklist item complete, and leaves
the user with a clean worktree for subsequent work.

## Problem Statement

The current delivery process can leave several conflicting statements of
reality: the implementation may be ready or a PR may exist, while the loop
checklist remains unchecked; a fix may land on another branch while the active
branch still fails the rubric; and externally located worktrees make the
delivery context hard to discover. Users must manually reconcile state,
commit bookkeeping, push, create branches, and write a PR summary.

## Goals

| Priority | Goal |
|---|---|
| P0 | Treat the active loop checklist item as the authoritative delivery record and complete it only after the verified PR creation succeeds. |
| P0 | Compare the active loop, current branch, PRD rubric verdict, and existing PR state before any completion mutation; report a mismatch and stop when they disagree. |
| P0 | Keep managed run worktrees under `<project>/.loopdeck/runs/`, with safe discovery/resume behavior for existing runs. |
| P0 | On a passing rubric result, create a normal commit, push it, and create a PR automatically. Include a concise rubric verdict in the commit and PR body. |
| P0 | After PR creation, update the matching loop checklist item and delivery state atomically enough to avoid visible drift, then prepare a clean next-work branch/worktree. |
| P0 | Make every successful and blocked delivery outcome understandable in the UI, including the reason and the next safe action. |
| P1 | Preserve a recoverable delivery record after a failed verify, commit, push, or PR operation so the user can retry without losing the branch. |

## Non-Goals

- Merge a pull request automatically.
- Rewrite Git history, force-push, or modify prior commits.
- Rewrite a different PRD without an explicit user request.
- Treat a passing fix on a different branch as evidence that the active loop is complete.
- Delete a worktree containing uncommitted work.

## Design

### Delivery gate

The delivery action resolves one active loop and its linked PRD/branch. It
performs the following ordered gates:

1. Confirm that the active loop and its checklist item are still pending.
2. Confirm the current worktree/branch is the one recorded for that loop.
3. Run the PRD rubric and retain its per-criterion report.
4. Refuse completion on a non-passing rubric result or when a PRD/branch/loop
   link is missing or conflicts. Report the exact conflicting records.
5. Create a normal commit whose message includes the rubric summary, push the
   branch, and create the PR with the same evidence.
6. Only after the PR is successfully created, mark the matching checklist item
   and loop complete. PR merge remains a manual user action.
7. Persist a handoff record, retain the delivered branch for review, and create
   or switch to a clean worktree/branch for the next loop.

Failures leave the current worktree intact. The UI reports whether nothing was
mutated, a local commit was made but push failed, or a branch was pushed but PR
creation failed, and offers the idempotent next action.

### Worktree location

New managed run worktrees live in `.loopdeck/runs/<branch-name>/`, mirroring
the branch they contain. Existing external
worktrees are discovered and retained; they are never implicitly deleted or
moved. A migration/reconciliation view identifies them and lets the user resume
or explicitly relocate them when safe.

### Checklist ownership

The checklist remains the only source that declares a loop completed. Delivery
metadata may reference a PR URL, branch, commit SHA, and rubric report, but it
must not independently imply completion. Any mismatch blocks automation and is
shown as a repairable discrepancy rather than silently rewritten.

## Phases

### Phase 1 — Reconciliation model and visible state

- [ ] `delivery-bookkeeping/reconciliation-model` Define persisted delivery links and mismatch states for a loop, branch, PRD, rubric result, and PR.
- [ ] `delivery-bookkeeping/reconciliation-report` Expose a user-facing verification and discrepancy report before delivery mutations.

### Phase 2 — Managed worktree containment

- [ ] `delivery-bookkeeping/runs-directory` Place newly created managed worktrees in `.loopdeck/runs/` and cover resume behavior.
- [ ] `delivery-bookkeeping/existing-worktree-safety` Detect external legacy worktrees without moving or deleting them automatically.

### Phase 3 — Verified delivery pipeline

- [ ] `delivery-bookkeeping/delivery-gates` Implement the branch, loop, PRD, and rubric gates that block mismatched or failing delivery.
- [ ] `delivery-bookkeeping/commit-push-pr` Commit and push a passing branch, then create a PR that includes the rubric result.
- [ ] `delivery-bookkeeping/complete-after-pr` Complete the matching checklist item only after PR creation succeeds and persist the delivery record.

### Phase 4 — Clean handoff and recovery

- [ ] `delivery-bookkeeping/clean-handoff` Prepare a clean next worktree/branch while retaining the delivered branch for review.
- [ ] `delivery-bookkeeping/retry-recovery` Make commit, push, and PR-creation failures recoverable and clearly actionable.

### Phase 5 — Verification

- [ ] `delivery-bookkeeping/delivery-integration-tests` Add coverage for success, a failing rubric, cross-branch fixes, push/PR failures, and legacy worktrees.
- [ ] `delivery-bookkeeping/prd-acceptance-audit` Verify the finished implementation against every P0 acceptance criterion with evidence.

## Open Questions

- Whether a clean next branch should be created eagerly after delivery or only when the user starts the next loop.
- Which remote-host PR provider(s) beyond GitHub should participate in automatic PR creation.
