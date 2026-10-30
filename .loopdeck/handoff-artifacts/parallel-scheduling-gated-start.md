---
artifact: parallel-scheduling-gated-start
author_role: planner
phase: parallel-scheduling/gated-start
type: report
created: 2026-10-30
summary: Dependent phases hold in Waiting until every upstream phase lands green; a non-green upstream fails its Waiting dependents while independent in-flight phases continue.
cites:
  - parallel-scheduling-parallel-batches
---

## Summary
`RunPhaseStatus::Waiting` is a real gated state. At run start `scheduler::mark_waiting` holds every queued phase with an unsatisfied dependency. `reconcile_gates` releases Waiting phases to Queued when all dependencies are green, and fails Waiting or Queued phases whose upstream ended Parked, Failed, Interrupted, or Killed. `fail_dependents` then cascades that failure to the rest of the chain. Running siblings are never touched.

## Requirements
- R1: A phase with an unsatisfied `depends_on` set is `Waiting`, not `Queued`, and is never selected for a turn.
- R2: When every dependency is Completed or Delivered, a Waiting phase becomes Queued and eligible immediately.
- R3: A non-green upstream fails its transitively dependent Waiting and Queued phases, and the payload names the upstream and its reason.
- R4: Independent in-flight phases continue after another phase parks, fails, or hits its budget.
- R5: `StallPolicy::Halt` additionally parks every remaining not-yet-started phase.
- R6: `transition_phase` accepts Queued→Waiting, Waiting→Queued, and Waiting→Running.

## Constraints
- C1: Gate reconciliation is pure and runs to a fixed point, so a failure can cascade within one pass.
- C2: A scheduler stall with no runnable phase fails the remaining phases instead of spinning.

## Non-Goals
- Releasing a dependent on a WARN or BLOCK verdict; only Completed or Delivered count as green.
- Interactive approval of a gated start.

## Open Questions
- Whether a UI control should let a user force-release a Waiting phase is deferred.
