---
artifact: parallel-scheduling-parallel-batches
author_role: planner
phase: parallel-scheduling/parallel-batches
type: report
created: 2026-10-30
summary: Phases with satisfied, disjoint depends_on run concurrently in separate worktrees under RunPlan.max_concurrency, default four and configurable in the picker.
cites:
  - parallel-scheduling-one-turn-per-phase
---

## Summary
A ready-set scheduler replaces topological batches. `scheduler::select_ready` returns queued phases whose dependencies are all green, capped by spare concurrency. `execute_run` prepares each one in its own worktree cut from the default branch, spawns a Tokio task, and joins completions with `JoinSet`. `RunPlan.max_concurrency` defaults to four and the plan-tonight wizard exposes it.

## Requirements
- R1: `RunPlan.max_concurrency` is persisted and defaults to four; values outside 1..=16 are rejected at plan creation.
- R2: `scheduler::select_ready` returns only queued phases whose every `depends_on` phase is Completed or Delivered.
- R3: The executor starts ready phases until `max_concurrency` turns are in flight; a completion immediately frees a slot.
- R4: Each phase worktree is cut from the repository default branch; no phase stacks on a sibling's branch.
- R5: Each phase opens its own draft PR from its own branch.
- R6: The wizard passes the concurrency value through `createRunPlan` and shows it in the review summary.

## Constraints
- C1: Worktree creation stays in the serialized scheduler loop, so concurrent `git worktree add` calls cannot race.
- C2: One worktree per phase keeps provider sessions keyed by path and prevents cross-phase session sharing.

## Non-Goals
- Auto-merging concurrent branches; arbitration remains `prd-arbitration`.
- Cross-project scheduling and global budgets.

## Open Questions
- Whether a provider-level rate limit should further cap effective concurrency is deferred.
