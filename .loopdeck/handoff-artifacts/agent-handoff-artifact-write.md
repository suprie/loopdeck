---
artifact: agent-handoff-artifact-write
author_role: planner
phase: agent-handoff/artifact-write
type: report
created: 2026-10-28
summary: The run executor copies each passing, handoff-opted phase's artifact into the store and parks completion when the configured slug or file is missing.
cites: []
---

## Summary
`create_run_plan` accepts an optional per-phase `handoff_topic`, validates it as a unique kebab-case slug, and persists it on `RunPhase`. At completion the executor appends an artifact instruction to the combined prompt only for phases that opted in, then validates and copies each source file into `.loopdeck/handoffs/`; a missing or mismatched artifact parks the batch. The executor normalizes the body into the contract's schema headings rather than trusting the producer.

## Requirements
- R1: The slug is supplied at queue time and stored on the phase; the executor never derives it from the execution id.
- R2: Topics are rejected when they are empty, non-kebab-case, reference an unknown phase, repeat a phase, or repeat a topic across phases.
- R3: After a passing turn, `emit_phase_artifacts` copies each opted-in phase's `.loopdeck/handoff-artifacts/<topic>.md` (or the public store path) into the persistent store.
- R4: A configured phase with no source file, a mismatched phase id, or an invalid record fails completion and parks the batch instead of silently passing.
- R5: Phases without a configured topic keep executing unchanged, preserving legacy plans (ADR-3).
- R6: The executor canonicalizes common heading aliases, demotes unknown top-level headings, and generates `## Summary` from the record when the producer omitted it.

## Constraints
- C1: The artifact source is read from the isolated worktree; the persistent store is written only in the project root.
- C2: An existing topic is never overwritten, so a retry cannot silently replace a published handoff.

## Non-Goals
- Prompt-side downstream injection and Waiting transitions (Phase 2), and handoff attribution in the run report (Phase 3).

## Open Questions
- Whether the phase should also declare its artifact `type` (currently every emitted artifact is `report`) is deferred to the injection phase.
