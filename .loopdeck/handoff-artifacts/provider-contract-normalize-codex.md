---
artifact: provider-contract-normalize-codex
author_role: senior-engineer
phase: provider-contract/normalize-codex
type: report
created: 2026-10-28
summary: Codex app-server traffic now normalizes to the same RunEvents as Claude, with capability gaps explicit and the legacy channel unchanged.
cites:
  - docs/epics/execution-foundations/provider-event-contract.md
  - src-tauri/tests/fixtures/provider-events/README.md
---

## Summary

`RunEventNormalizer::convert_codex_message` maps app-server notifications to
the shared payload table: `turn/started` → started, text/reasoning deltas →
content, `item/started` tools → tool, token usage → usage, and
`turn/completed` → exactly one terminal. Completed agent messages are
suppressed when deltas already carried the text, so Codex and Claude produce
the same semantic content sequence. Server requests emit pending decisions;
the adapter emits resolved decisions from the same policy path that writes the
wire response (auto-allow included).

## Constraints

- C1 — Legacy `Channel<ClaudeEvent>` payloads are unchanged; normalized events
  are emitted on the additive stream from the same read loop.
- C2 — A Codex plan request is not a real capability: it becomes
  `unsupported: plan_approval`, never a relabelled approval.
- C3 — Dynamic client-declared tool calls and unknown server requests become
  explicit `unsupported` events.
- C4 — One terminal per segment, gated in the normalizer; adapter errors map
  to LoopDeck-owned outcomes (budget → killed, parked deadline → cancelled).

## Non-Goals

- No new Codex protocol capabilities.
- No change to the legacy multi-agent event channel.
- No durable persistence of normalized events yet.

## Open Questions

- Q1 — Multi-agent workers do not attach a normalized stream yet; the run queue
  is the only migrated producer. Extend in the replay phase if the morning
  report needs per-worker normalized history.
- Q2 — Codex usage normalizes the turn-level `last` counters. Revisit whether
  cumulative `total` should be persisted separately once usage reporting is
  durable.
