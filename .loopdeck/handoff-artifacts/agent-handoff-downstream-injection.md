---
artifact: agent-handoff-downstream-injection
author_role: planner
phase: agent-handoff/downstream-injection
type: report
created: 2026-10-30
summary: Dependent phases now receive their upstream artifacts inline, bounded to 8 KiB, in a separate turn with a required citation block.
cites: []
---

## Summary

A phase that directly depends on a phase with a `handoff_topic` now runs in its own turn. That turn inlines each upstream artifact from `.loopdeck/handoffs/`, bounded to 8 KiB per artifact, and names the store path. Phases without a publishing ancestor still batch exactly as before.

## Requirements

1. `next_queued_batch` stops a batch before a phase whose direct `depends_on` ancestor publishes a handoff topic.
2. The consumer prompt inlines each upstream artifact body and its store path.
3. Injection is bounded at 8 KiB per artifact; an oversized or missing artifact parks the batch, never truncates.
4. The prompt requires a `## Handoff citations` block keyed by section heading and item ID.

## Constraints

1. Injection reads the persistent store in the main checkout, not the run worktree snapshot.
2. Only direct `depends_on` edges define the turn boundary.
3. Legacy plans without `handoff_topic` keep their previous combined-turn behavior.

## Non-Goals

No message bus, no parallel execution, and no change to artifact authoring beyond the existing Phase 1 contract.

## Open Questions

None. The queue-time clarification chose the split-for-consumers rule.
