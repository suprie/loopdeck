---
artifact: parallel-scheduling-one-turn-per-phase
author_role: planner
phase: parallel-scheduling/one-turn-per-phase
type: report
created: 2026-10-30
summary: The run executor replaces the combined queued-batch turn with one independent turn per run-plan phase, each in its own worktree and branch.
cites: []
---

## Summary
`execute_run` no longer calls `next_queued_batch` or `build_combined_phase_prompt` with many loops. It prepares each ready phase as its own `PhaseTask`, builds a single-loop prompt for it, spawns one provider turn, and records that phase's own verdict, tokens, and wall clock. The plan is mutated only by the serialized scheduler loop.

## Requirements
- R1: Each run-plan phase executes in exactly one executor turn; no turn covers two phases.
- R2: Each phase gets its own prompt built from one loop location, its pinned interview answers, and its handoff topic.
- R3: Each phase records its own token usage and wall-clock seconds; nothing is divided across a batch.
- R4: The phase prompt names its own branch and forbids merging another phase's branch.
- R5: Only the scheduler loop writes `run-plan.yaml`; phase turns run in Tokio tasks and touch no plan state.
- R6: A missing `execution.yaml` focus slot is skipped, not treated as an error; `run-plan.yaml` stays authoritative per phase.

## Constraints
- C1: `AppState.claude_sessions` is keyed by worktree path, so one worktree per phase keeps concurrent turns isolated.
- C2: Pending question, permission, plan, and interrupt slots use the phase worktree as the control key.

## Non-Goals
- Concurrent fan-out and the Waiting gate, which are `parallel-scheduling/parallel-batches` and `gated-start`.
- Changes to the one-run-per-project admission lock.

## Open Questions
- Whether a phase should inherit an interactive `execution.yaml` current pointer when it is not the first started is left to the UI-focus design.
