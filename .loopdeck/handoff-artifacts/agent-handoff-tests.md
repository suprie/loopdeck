---
artifact: agent-handoff-tests
author_role: planner
phase: agent-handoff/tests
type: report
created: 2026-10-30
summary: Rust tests cover store round-trips, the 8 KiB injection bound, waiting transitions, batch boundaries, and citation coverage against the spike contract.
cites: []
---

## Summary

The test suite now covers the handoff path end to end without a live provider. Tests cover the store and ledger round-trips, the artifact injection size bound, the Waiting transition edges, the batch split at a consumer, and the citation coverage rules from the spike contract.

## Requirements

1. Store and ledger round-trips, including backward-compatible ledgers.
2. Injection rendering names the store path, author role, and citation rule.
3. Injection accepts an 8 KiB body and rejects a larger one without truncation.
4. Waiting transitions cover queued to waiting, waiting to queued, and waiting to parked.
5. Re-check tests cover complete, terminal-bad, and chained dependencies.
6. Restart reconciliation re-queues waiting phases.
7. Batch tests prove a consumer always starts a new turn.
8. Citation tests cover pass, partial, missing, and not-used classification.

## Constraints

1. Every test is deterministic and local; no provider or network access.
2. Tests use temporary directories and remove them after each run.

## Non-Goals

No load or performance testing, and no live end-to-end model session.

## Open Questions

None.
