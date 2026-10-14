---
prd: prd-run-event-replay
epic: execution-foundations
milestone: "unassigned"
status: proposed
description: >
  Persist ordered run events and let the UI replay from a cursor before
  resuming live updates, so navigation and restart preserve the same story.
order: 40
---

# PRD — Run Event Replay

## Overview

Use the provider-neutral event vocabulary and operational store to make live
updates replayable. The backend is the source of truth; frontend state is a
projection keyed by run ID and last seen sequence.

## Problem Statement

The current channel carries live events, while canonical transcript turns
are loaded separately. Navigation, remount, and restart require special UI
handling and can lose or duplicate in-flight content.

## Goals

| Priority | Goal |
|---|---|
| P0 | Append durable events with monotonic per-run sequence numbers and exactly one terminal outcome per run segment. |
| P0 | Expose snapshot-plus-events-since-cursor and a live subscription with an explicit no-gap handoff. |
| P0 | Make the UI reducer idempotent across replay, live delivery, remount, and restart. |
| P1 | Retain legacy transcript read/export and define event retention plus compaction without losing user-visible history. |

## Non-Goals

- Store every raw token delta forever; durable content may be coalesced at a
  documented boundary.
- Redesign the entire chat UI in this PRD.
- Replace provider-native transcript files needed for resume until their
  compatibility role is proven unnecessary.

## Design

Use `(run_id, sequence)` as the replay cursor and a stable event ID for
deduplication. Write terminal and decision events durably before signaling
the UI. A reconnect asks for a snapshot and all later events, then attaches
to the live stream without a gap; the test must deliberately publish an event
during handoff. The frontend derives current tasks, approvals, and transcript
rows from the same ordered reducer. Keep existing legacy transcript export
available during migration.

## Phases

### Phase 1 — Durable event path

- [ ] `run-event-replay/append-events` Persist normalized events with per-run ordering, redaction, schema version, and duplicate-event rejection.
- [ ] `run-event-replay/read-cursor` Add snapshot and events-since-cursor commands with bounded pagination and explicit stale-cursor behavior.

### Phase 2 — Live handoff and UI projection

- [ ] `run-event-replay/subscribe-gapless` Add live subscription handoff that cannot miss an event between replay and subscribe; cover reconnect and multiple viewers.
- [ ] `run-event-replay/reduce-interactive` Replace interactive runs’ transient channel-only projection with an idempotent run-ID keyed reducer and prove parity against the current transcript.
- [ ] `run-event-replay/reduce-queue` Migrate queued-run projection to the reducer and prove parity against current phase status and morning report.
- [ ] `run-event-replay/reduce-multi-agent` Migrate multi-agent projection to the reducer and prove parity against existing manifests.
- [ ] `run-event-replay/preserve-export` Keep existing transcript read/export and document retention and compaction for the new event history.

### Phase 3 — Verification

- [ ] `run-event-replay/test-reconnect` Test event during handoff, duplicate delivery, out-of-order rejection, navigation, restart, and terminal-result uniqueness for both providers.
- [ ] `run-event-replay/test-render` Render a user turn and approval from replay plus live updates and assert each appears exactly once.

## Acceptance Criteria

- A client disconnected at any event cursor reconstructs the same final run
  snapshot as a continuously connected client.
- Navigation and restart preserve pending decisions and completed content;
  stale `busy` state never survives a dead process.
- A rendered user turn, approval, and terminal result each appear once when
  replay overlaps with live delivery.
- Existing transcript history and export stay available during migration.

## Open Questions

- Which high-volume deltas should be coalesced before persistence? Decide from
  measured event volume and an explicit user-visible fidelity requirement.
