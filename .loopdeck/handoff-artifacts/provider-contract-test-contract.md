---
artifact: provider-contract-test-contract
author_role: senior-engineer
phase: provider-contract/test-contract
type: report
created: 2026-10-28
summary: Fixture parity tests for both providers and a node:test projection test prove one terminal per segment, preserved approval payloads, and typed bindings; no DOM render runner exists.
cites:
  - .loopdeck/handoff-artifacts/provider-contract-normalize-claude.md
  - .loopdeck/handoff-artifacts/provider-contract-normalize-codex.md
  - .loopdeck/handoff-artifacts/provider-contract-share-types.md
  - src-tauri/tests/fixtures/provider-events/README.md
---

## Summary

`src-tauri/src/provider_contract_tests.rs` normalizes every paired fixture
fragment and asserts semantic parity: declared payloads, equivalent text words
and token counts, matching decision kinds/states, and exactly one terminal per
segment. Replaying a provider terminal produces no second terminal. Approval
fixtures keep tool name plus command, and question fixtures keep the structured
question text and options in `detail_redacted`. On the frontend,
`tests/frontend/runEventProjection.test.mjs` exercises the pure reducer for
content coalescing, tool rows, pending/resolved decisions, terminal synthesis,
retry, duplicate-terminal idempotence, and unsupported gaps. `tsc` checks the
reducer against the generated bindings.

## Constraints

- C1 — No new frontend dependencies; the test uses `node:test` and
  `node --experimental-strip-types`, matching the existing suite.
- C2 — No DOM render test runner is installed, so the "render test" is covered
  at the pure projection seam; this gap is explicit, not implied-pass.
- C3 — Contract tests read sanitized fixtures only and never live captures.
- C4 — Tests assert product-level semantics, not wire syntax or chunking.

## Non-Goals

- No visual screenshot assertions.
- No live Claude/Codex session in CI.
- No schema-fuzzing or property-based tests in this pass.

## Open Questions

- Q1 — Should the replay PRD add a jsdom/Playwright smoke for the actual
  transcript component? Recommended once the event store persists history.
- Q2 — `usage` parity compares the turn-level counters; cumulative thread
  usage remains untested until it becomes durable product state.
