---
prd: prd-domain-contract
epic: execution-foundations
milestone: "unassigned"
status: proposed
description: >
  Define run identity, transition rules, data ownership, and the evidence gate
  for moving operational state while preserving existing project files.
order: 10
---

# PRD — Domain Contract

## Overview

Produce the contract that the other four PRDs implement. This PRD is complete
only when the contract is checked against the current command and persistence
paths, not merely written as a diagram.

## Problem Statement

Execution state, run plans, transcripts, session slots, and UI streaming state
can describe the same run at different times. A migration without an explicit
owner and transition table would add another source of disagreement.

## Goals

| Priority | Goal |
|---|---|
| P0 | Inventory every persisted and in-memory field that represents run, phase, session, approval, or delivery status, with its writer and reader. |
| P0 | Define stable IDs, allowed transitions, invariants, and crash reconciliation for interactive, queued, and multi-agent runs. |
| P0 | Decide operational-store choice from measured constraints; define backup, compatibility, and rollback before code migration. |
| P1 | Add a small pure Rust transition layer and tests, then route one low-risk command through it without changing observable behavior. |

## Non-Goals

- Migrate persisted data or replace the session registry in this PRD.
- Change run scheduling, approval policy, or delivery verification.
- Rename all existing public IPC types in one pass.

## Design

The contract must include: `ProjectId`, `RunId`, `PhaseId`, `SessionId`, and
`DecisionId`; a path as a mutable project locator; transition preconditions
and terminal states; `interrupted` recovery; idempotency keys for retries; and
the exact owner of each mutable field. Preserve existing stable phase IDs from
the spec layer. Use an explicit mapping for legacy runs that have no run ID.
Document where the current `execution.yaml` revision and `run-plan.yaml`
status fields remain authoritative during each migration stage.

## Phases

### Phase 1 — Inventory and decision gate

- [x] `domain-contract/inventory-state` List every run-related writer, reader, file, in-memory map, and IPC command; mark conflicting ownership and overlap with the optimization epic. See [domain-contract-inventory.md](domain-contract-inventory.md).
- [x] `domain-contract/measure-store` Compare SQLite with the current file approach using a real sample of run/event volume, concurrent writers, backup/restore, and cross-platform packaging; record the decision and rejection criteria. See [operational-store-decision.md](operational-store-decision.md).
- [x] `domain-contract/map-migration` Document legacy-to-new ID mapping, compatibility reads, rollback artifact, and a pilot project fixture; identify any data that cannot be reconstructed. See [migration-map.md](migration-map.md).

### Phase 2 — Transition contract

- [x] `domain-contract/define-transitions` Write the state transition table for interactive, queued, and multi-agent runs, including parked, cancelled, killed, interrupted, delivered, and completed outcomes. See [transition-contract.md](transition-contract.md).
- [x] `domain-contract/implement-transitions` Add pure Rust transition functions with structured invalid-transition errors; route one low-risk command through them without changing on-disk format. See `src-tauri/src/transition.rs` and restart reconciliation in `src-tauri/src/run_executor.rs`.

### Phase 3 — Verification

- [x] `domain-contract/test-transitions` Add table-driven tests for every allowed and rejected transition, duplicate command, and restart reconciliation rule; verify existing commands still return compatible results. See transition tests in `src-tauri/src/transition.rs` and restart/idempotency coverage in `src-tauri/src/run_executor.rs`.

## Acceptance Criteria

- Every run-related mutable field in the inventory has exactly one named
  writer and one authoritative read path at each migration stage.
- The store decision includes measured evidence and an executable rollback
  procedure; no persistence PRD starts before it is accepted.
- The pure transition tests cover all terminal statuses and reject at least
  one invalid edge per state family.

## Open Questions

- Which current file fields must remain portable across machines, beyond
  authored intent and delivery links? Resolve in Phase 1.
