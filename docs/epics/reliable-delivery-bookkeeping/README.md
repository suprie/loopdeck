---
title: Reliable Delivery Bookkeeping
slug: reliable-delivery-bookkeeping
milestone: "unassigned"
status: proposed
started: 2026-08-31
owner: Suprie
description: >
  Let a user finish a PR-backed loop with an accurate, reconciled checklist and
  a clean worktree ready for the next loop, so the todo remains the single
  source of truth rather than drifting behind the branch and PR state.
---

# Epic — Reliable Delivery Bookkeeping

## Motivation

Loop completion currently leaves manual bookkeeping behind: a PR can be
created or merged while its loop remains incomplete in the todo/checklist.
The resulting drift makes it unclear whether a branch, PRD, and checklist
describe the same work. LoopDeck should reconcile that evidence, report any
conflict, and leave the user on a clean worktree for the next loop.

## Scope

- Keep the loop todo/checklist as the single source of truth for delivery state.
- Store LoopDeck worktrees beneath the project at `.loopdeck/runs/` rather than
  a sibling `../.loopdeck-runs` directory.
- Verify the current branch against its PRD using the rubric and report the
  verdict to the user before delivery actions.
- Reconcile branch, PR, and checklist state; surface conflicts instead of
  silently treating a failed branch as complete because a fix exists elsewhere.
- Commit the approved bookkeeping and implementation changes, then push the
  branch automatically.
- Create a pull request with rubric evidence included in the commit/PR message.
- Clean up delivery state and prepare a clean branch/worktree for the next loop.

## Non-Goals

- Merge a pull request automatically.
- Rewrite Git history.
- Rewrite another PRD unless the user explicitly requests it.

## PRD Index

| PRD | Status | Description |
|---|---|---|
| [Verified delivery and reconciliation](prd-verified-delivery-reconciliation.md) | proposed | Reconcile loop state with branch and PR evidence, then automate a verified handoff. |

## Architecture Decisions

### ADR-1: Reconciliation authority — fill in

## Success Criteria

- After delivery, the user has a clean branch ready for the next loop.
- The loop status and checklist update automatically, remaining the single source of truth.
- A verification report explains the branch's rubric result before it is committed, pushed, or proposed for review.
- When the branch, PRD, and checklist disagree, LoopDeck reports the conflict and does not falsely record the loop as complete.

## Risks

| Risk | Mitigation |
|---|---|
| A failing branch is fixed from another branch while the PRD/checklist remains stale, producing contradictory delivery evidence. | Require an explicit reconciliation pass that compares the current branch, linked PR, PRD rubric result, and loop identity; block automatic completion on mismatch and present the discrepancy to the user. |
