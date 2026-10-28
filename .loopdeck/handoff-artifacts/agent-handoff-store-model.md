---
artifact: agent-handoff-store-model
author_role: planner
phase: agent-handoff/store-model
type: report
created: 2026-10-28
summary: Handoff artifacts stay Markdown at .loopdeck/handoffs/<topic>.md, with a separate plain-JSON ledger holding graph nodes and links.
cites: []
---

## Summary
`handoff.rs` now owns the full store contract: the artifact record (topic, author role, phase, type, created, summary, cites, durable path), strict load/save helpers with 1 KiB frontmatter and 8 KiB body caps, and an optional duplicate-topic refusal so a retry never overwrites the original. The graph-native ledger (`ledger.json`) records one node per artifact and one link per citation, per the run's clarification.

## Requirements
- R1: Artifacts live at `.loopdeck/handoffs/<topic>.md`; the topic is a stable kebab-case slug that matches the filename.
- R2: `ArtifactRecord` carries `artifact`, `author_role`, `phase`, `type`, `created`, `summary`, optional `cites`, and the durable `path` (derived, never duplicated in frontmatter).
- R3: `load_artifact`/`save_artifact` validate the contract, enforce the size caps, and refuse to replace an existing topic.
- R4: `save_artifact` appends a `HandoffLedgerNode` and, for each citation, a `cites` `HandoffLedgerLink` into `.loopdeck/handoffs/ledger.json`.
- R5: `load_ledger` returns an empty graph when the file is absent and a structured `RunPlan` error when it is malformed JSON.

## Constraints
- C1: Markdown artifacts are append-only in spirit; headings and item IDs are never renamed.
- C2: Ledger nodes and links are plain JSON so they can be queried independently of the Markdown bodies.

## Non-Goals
- Downstream artifact injection, Waiting status, and report attribution, which are Phase 2-3 loops.

## Open Questions
- Retention policy across runs remains open in the PRD; the ledger currently grows append-only.
