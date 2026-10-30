---
artifact: agent-handoff-report-attribution
author_role: planner
phase: agent-handoff/report-attribution
type: report
created: 2026-10-30
summary: Every consumed handoff is recorded in the project ledger with run id, roles, artifact path, and a coverage-only citation result, then filtered into the run report.
cites: []
---

## Summary

The handoff ledger gains one entry per consumed artifact. Each entry records the run id, the producing role, the consuming role, the store path, and a coverage-only citation result. The morning report reads the project-wide ledger and filters it by the current plan id.

## Requirements

1. Extend the ledger with a `handoffs` list of typed entries.
2. Record an entry after a passing consumer turn for every injected artifact.
3. Parse the consumer's `## Handoff citations` block and classify each heading and numbered item as cited, `not-used`, or missing.
4. Roll coverage up to `pass`, `partial`, or `missing`.
5. Filter ledger entries by `run_id == plan.id` when building the run report.

## Constraints

1. The citation check judges coverage only, never prose fidelity or completeness.
2. Older ledgers without a `handoffs` field still load.
3. A failed ledger write logs a warning; it never blocks delivery.
4. Not-used lines may name a bare item ID, while cited lines use the path-and-heading form.

## Non-Goals

No arbitration, no artifact review, and no second opinion on the consumer's wording.

## Open Questions

None. The queue-time clarifications fixed run-id tagging and coverage-only checking.
