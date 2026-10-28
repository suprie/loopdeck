---
artifact: provider-contract-share-types
author_role: senior-engineer
phase: provider-contract/share-types
type: report
created: 2026-10-28
summary: RunEvent TypeScript bindings are generated with ts-rs, committed, CI-drift-checked, and one consumer now reads the normalized stream through a pure reducer.
cites:
  - .loopdeck/handoff-artifacts/provider-contract-normalize-claude.md
  - .loopdeck/handoff-artifacts/provider-contract-normalize-codex.md
  - docs/epics/execution-foundations/provider-event-contract.md
---

## Summary

`provider_event.rs` derives `ts_rs::TS`; `cargo test export_bindings` writes
`src/types/generated/*.ts`, re-exported by `src/types/runEvent.ts`. CI
regenerates and runs `git diff --exit-code -- src/types/generated`, so Rust and
TypeScript cannot drift silently. The run-queue hook migrated from the legacy
`run-queue-event` `ClaudeEvent` payload to the normalized `run-event` topic.
All interpretation is in the pure `src/lib/runEventProjection.ts` reducer; the
hook only maps its projection onto the existing Zustand stores.

## Constraints

- C1 — Generated files are never hand-edited; the Rust types are the source of
  truth.
- C2 — Token counts and the optional sequence are generated as `number`, not
  `bigint`, so existing render code needs no conversion.
- C3 — The legacy `run-queue-event` topic keeps emitting unchanged during the
  dual-emit phase.
- C4 — The migrated consumer preserves the existing store shape, so Chat,
  TaskPanel, and pending-card surfaces render without changes.

## Non-Goals

- No removal of `ClaudeEvent` or the hand-maintained legacy type.
- No new frontend dependency (no DOM test runner, no schema generator beyond
  ts-rs).
- No replay/cursor work; ordering is still live-stream order.

## Open Questions

- Q1 — `detail_redacted` is a JSON string rather than a typed union. Typing it
  per decision kind would be a contract change for the replay PRD.
- Q2 — The projection synthesizes a legacy-shaped `result` object for the
  terminal event; retire it when render components consume `RunEvent` directly.
