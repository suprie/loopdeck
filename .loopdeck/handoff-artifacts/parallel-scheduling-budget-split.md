---
artifact: parallel-scheduling-budget-split
author_role: planner
phase: parallel-scheduling/budget-split
type: report
created: 2026-10-30
summary: Token and wall-clock budgets apply per in-flight phase, so a budget kill ends only that phase and independent phases keep running.
cites:
  - parallel-scheduling-one-turn-per-phase
---

## Summary
Each `PhaseTask` gets its own `TokenBudget` sized from `RunBudgets.per_phase_token_cap`, with no multiplication by batch size. Its watchdog is `min(phase wall-clock cap, remaining run wall-clock)`. `resolve_phase_token_cap` and `resolve_phase_timeout` keep their per-phase meaning. Token or wall-clock exhaustion marks only that phase Killed, fails its dependents, and lets siblings continue.

## Requirements
- R1: Every phase turn owns a separate `TokenBudget` built from the per-phase cap.
- R2: The per-phase wall-clock watchdog is the smaller of the phase cap and the run's remaining cap.
- R3: Exceeding either cap marks only that phase Killed with the budget message.
- R4: A budget kill fails the phase's dependents and does not stop independent in-flight phases.
- R5: The run-level wall-clock cap still stops every unfinished phase when it expires.
- R6: Per-phase token usage and wall-clock seconds are recorded on the phase for the morning report.

## Constraints
- C1: Budget enforcement reuses the existing `TokenBudget` and watchdog kill mechanics; no new timeout machinery.
- C2: The run-level cap remains a hard ceiling across all concurrent phases.

## Non-Goals
- Per-phase cap overrides distinct from the plan-level value.
- Global or cross-project token budgets.

## Open Questions
- Whether a phase should receive a partial refill when a sibling frees capacity is deferred.
