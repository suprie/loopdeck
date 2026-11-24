---
artifact: parallel-scheduling-tests
author_role: planner
phase: parallel-scheduling/tests
type: report
created: 2026-10-30
summary: Scheduler property tests cover diamond graphs, cycle rejection, restart recovery, gates, and worktree cleanup isolation; the full suite passes.
cites:
  - parallel-scheduling-parallel-batches
  - parallel-scheduling-gated-start
  - parallel-scheduling-restart-reconcile
---

## Summary
`src-tauri/src/scheduler.rs` ships fourteen unit tests: diamond validation and ordering, gate release only after both parents, transitive dependent failure, independent in-flight survival, Halt parking, cycle and dangling-edge rejection, concurrency caps, watchdog selection, restart decisions, branch uniqueness, and stall detection. `commands/run_queue.rs` adds worktree tests: disjoint phase paths, isolated cleanup, resume, recreation from a surviving branch, and default-branch basing.

## Requirements
- R1: A diamond graph validates, orders roots first, and releases its leaf only when both parents are green.
- R2: A cycle, a self-dependency, and a dangling edge are rejected before any worktree exists.
- R3: A non-green upstream fails transitively dependent phases while an independent in-flight phase keeps Running.
- R4: Restart reconciliation auto-resumes only when worktree and session both survive; otherwise it parks with a reason.
- R5: Two phase worktrees are disjoint, and removing one never touches its sibling.
- R6: The full Rust library suite and the frontend test suite pass.

## Constraints
- C1: Scheduler tests are pure and need no provider process or Tauri state.
- C2: Worktree tests use scratch git repositories under the system temp directory.

## Non-Goals
- Live provider turns in the automated suite; the role demo remains an explicitly ignored test.
- Load or soak testing above four concurrent phases.

## Open Questions
- Whether the ignored role demo should become a scheduled integration test is deferred.
