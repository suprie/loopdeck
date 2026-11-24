---
artifact: parallel-scheduling-restart-reconcile
author_role: planner
phase: parallel-scheduling/restart-reconcile
type: report
created: 2026-10-30
summary: Restart reconciliation handles multiple in-flight phases, auto-resuming each phase whose worktree and saved provider session survive and parking the rest with a reason.
cites:
  - parallel-scheduling-one-turn-per-phase
---

## Summary
`reconcile_running_phases` now classifies every `Running` phase instead of downgrading all of them to Interrupted. `scheduler::classify_interrupted_phase` requires both the phase worktree and a saved session id. A survivor returns to `Queued` with `resume_session_id` set; the executor spawns with `--resume` in the same worktree. Anything less parks with a reason that names what was lost.

## Requirements
- R1: Every `Running` phase on disk is reconciled on the next queue attempt or status read.
- R2: A phase whose worktree exists and whose session id is saved returns to Queued with that session pinned for resume.
- R3: The resumed turn spawns in the recorded worktree with the saved provider session.
- R4: A phase missing either survivor becomes Parked, and the payload names the missing piece.
- R5: Legacy plans fall back to the plan-level worktree and the active transcript's last session id.
- R6: `transition_phase` accepts Running→Queued for this auto-resume edge.

## Constraints
- C1: The decision is pure (`classify_interrupted_phase`) and unit-tested independently of the filesystem.
- C2: A stale `execution.yaml` current pointer left on an auto-resumed phase is abandoned so queueing can proceed.

## Non-Goals
- Resuming a provider process across an app restart; a new process resumes the saved session id.
- Automatic retry of a parked restarted phase.

## Open Questions
- Whether a resumed phase should replay its last partial turn or start a fresh instruction is still provider-defined.
