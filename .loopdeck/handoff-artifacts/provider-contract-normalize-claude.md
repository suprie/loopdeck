---
artifact: provider-contract-normalize-claude
author_role: senior-engineer
phase: provider-contract/normalize-claude
type: report
created: 2026-10-28
summary: Claude output now dual-emits provider-neutral RunEvents while the legacy ClaudeEvent channel stays byte-compatible.
cites:
  - docs/epics/execution-foundations/provider-event-contract.md
  - src-tauri/tests/fixtures/provider-events/README.md
---

## Summary

`src-tauri/src/provider_normalize.rs` converts each Claude `stream-json` line at
the adapter boundary into the versioned `RunEvent` envelope, and
`ClaudeSession::send_message_streaming` emits both streams from the same read
loop. Legacy `Channel<ClaudeEvent>` payloads are unchanged. Control requests
produce a pending decision, and the existing answer methods emit the resolved
decision (allowed/denied/expired). A per-segment terminal gate makes duplicated
provider terminals and synthesized interrupt terminals no-ops.

## Constraints

- C1 — The legacy channel remains the UI contract until the replay migration;
  no `ClaudeEvent` variant or field changed.
- C2 — Exactly one durable terminal per run segment; `Retrying` remains
  transient and a retry attempt is a new segment.
- C3 — Tool inputs, decision summaries/details, and terminal messages pass
  through `secret_scan::redact_secrets` and a 4 KB truncation before becoming
  product state.
- C4 — `auto-allow` maps to `DecisionState::Allowed` with the stable
  `auto-allow: ` summary prefix; no enum change.

## Non-Goals

- No durable event store or sequence assignment in this phase.
- No removal of legacy Claude consumers.
- No change to the permission policy or consent posture.

## Open Questions

- Q1 — Interactive streaming commands do not expose the normalized channel yet;
  only the detached run queue attaches it. Decide the interactive identity
  (`interactive-*` run id vs persisted conversation id) in the replay phase.
- Q2 — `raw_ref` is left empty because a payload digest would add a new hashing
  dependency; provider event IDs can be added when the event store lands.
