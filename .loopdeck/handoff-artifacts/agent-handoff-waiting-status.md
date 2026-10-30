---
artifact: agent-handoff-waiting-status
author_role: planner
phase: agent-handoff/waiting-status
type: report
created: 2026-10-30
summary: Run-plan phases gain a Waiting status for unmet handoff dependencies, re-checked after every turn and parked only when upstream fails or the run ends.
cites: []
---

## Summary

`RunPhaseStatus` now has a `Waiting` state. A consumer phase waits while its publishing ancestor is not `Completed` or `Delivered`. The executor re-checks waiting phases after every turn. A satisfied dependency re-queues the phase; a terminal-bad upstream parks it; a run that ends parks any remainder.

## Requirements

1. Add `Waiting` to `RunPhaseStatus` and the transition table.
2. Re-check blocked phases at the top of every executor iteration.
3. Re-queue a waiting phase when all publishing ancestors are `Completed` or `Delivered`.
4. Park a waiting phase when any publishing ancestor is `Parked`, `Failed`, `Interrupted`, or `Killed`.
5. Park remaining waiting phases on cancel, budget kill, or normal run end.
6. Re-queue waiting phases on restart reconciliation so a crash cannot strand them.

## Constraints

1. Waiting is distinct from Parking: it is a scheduler hold, not a human decision.
2. The morning report shows a separate `WAITING` verdict.
3. Waiting is never a terminal state and never implies success or failure.

## Non-Goals

No change to `MultiAgentRunStatus`, which already had its own `Waiting` value.

## Open Questions

None. The queue-time clarification fixed Waiting in the run plan and defined its ending conditions.
