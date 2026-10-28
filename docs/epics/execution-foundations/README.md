---
title: Execution Foundations
slug: execution-foundations
milestone: "unassigned"
status: proposed
started: 2026-10-13
owner: Suprie
description: >
  Let a user leave and reopen LoopDeck during an agent run, then see the same
  authoritative progress, pending decisions, and delivery result without
  guessing which transcript or status panel is current.
---

# Epic — Execution Foundations

## Motivation

LoopDeck grew from repository discovery into an agent runner with interactive
sessions, overnight queues, multi-agent work, and delivery. These features now
span `.loopdeck/execution.yaml`, `.loopdeck/run-plan.yaml`, session JSONL,
in-memory session maps, Tauri channels, and frontend streaming state. Each has
valid reasons to exist, but their ownership and recovery rules are scattered.
This epic gives a run one identity, one transition contract, and a replayable
history while preserving the working product throughout migration.

This is an incremental modernization, not a second implementation of LoopDeck.
Every PRD has a narrow migration boundary and a rollback path. Existing
`optimization/prd-session-state-consolidation` remains responsible for its
already-scoped in-memory lock cleanup; this epic must consume that result or
coordinate with it, not independently refactor the same fields.

## Scope

- Define stable identities, state transitions, ownership, and recovery rules
  for projects, runs, phases, sessions, approvals, and delivery evidence.
- Validate a local operational store for mutable run state and event history;
  keep portable authored specs and decisions in repository files.
- Normalize Claude and Codex output at the provider boundary and make the
  shared event contract explicit across Rust and TypeScript.
- Persist run events with ordered replay so the UI can reconnect after
  navigation or app restart without inventing missing in-flight state.
- Build an attention-focused project view using the authoritative run model.
- Add migration, restart, and rendered-UI verification at the boundaries that
  have previously produced state drift or duplicate content.

## Non-Goals

- Rebuild the app, change desktop framework, or replace the current UI at once.
- Move human-authored epics, PRDs, decisions, or project memory out of git.
- Add cloud sync, remote execution, or automatic PR merge.
- Change agent permission or consent policy as part of the data migration.
- Launch new agent capabilities before the underlying state is reliable.

## PRD Index

| PRD | Order | Reviewable result |
|---|---:|---|
| [Domain contract](prd-domain-contract.md) | 1 | State/ownership contract, compatibility map, and a migration gate |
| [Provider contract](prd-provider-contract.md) | 2 | Provider-neutral events and contract tests for Claude and Codex |
| [Operational store](prd-operational-store.md) | 3 | Durable run state with migration and restart recovery |
| [Run event replay](prd-run-event-replay.md) | 4 | Ordered event history and reconnecting UI |
| [Attention view](prd-attention-view.md) | 5 | One project summary and actionable pending decisions backed by the new read model |

**Dependency order:** 1 defines IDs and ownership; 2 fixes the event vocabulary;
3 persists run identity and transitions; 4 persists and replays the events; 5
projects the resulting state into the UI. Do not start a dependent PRD until
its predecessor's P0 acceptance checks pass. Small independent work in the
existing optimization epic may proceed separately.

## Architecture Decisions

### ADR-1: Operational store choice — SQLite with compatibility exports

Phase 1 evidence accepts SQLite for local mutable run state and event history,
provided the import is provenance-tracked, compatibility exports remain
readable, and backup/restore is verified before cutover. Repository files
remain authoritative for authored specs, decisions, and stable phase IDs.
The evidence and rollback procedure live in
[operational-store-decision.md](operational-store-decision.md).

### ADR-2: Repository and runtime ownership

The contract in `prd-domain-contract` will name one writer and one read model
for every mutable field. A path locates a project; a stable ID identifies it.
Delivery remains gated by existing verification, consent, and PR evidence.

## Success Criteria

- A run started on either provider keeps the same run ID and terminal outcome
  across navigation and app restart; a restart never displays `running` for a
  process that no longer exists.
- Given an event cursor, the UI can replay missed events in order and resume
  live updates without duplicate user turns, approvals, or terminal results.
- Every mutating run command uses the documented state transitions and rejects
  an invalid transition with a structured error; a test covers each terminal
  outcome and restart reconciliation.
- Authored `.loopdeck/` files remain readable by existing supported workflows
  during migration, and a tested backup/rollback route is documented before
  any old operational field is retired.
- A user can open one project view and identify the active run, the next
  decision requiring them, and verified delivery evidence without comparing
  multiple status surfaces.
- Existing Rust, frontend, and build checks pass at every migration checkpoint;
  new contract and UI tests cover the added behavior.

## Risks

| Risk | Mitigation / gate |
|---|---|
| Two writers disagree during file-to-store migration. | Inventory writers first; add one authoritative write path per field; compare old/new read models during a bounded compatibility period; reject divergence before removing legacy writes. |
| A new local database harms repo portability or recovery. | Keep authored files in git, export/backup operational data, test cold-start and restore, and record a go/no-go decision before implementation. |
| Provider events lose meaning when normalized. | Keep raw provider payloads available for diagnostics; use golden fixtures for both providers and assert approval, tool, usage, and terminal semantics. |
| Event replay duplicates or reorders UI content. | Use monotonic per-run sequence numbers, idempotent reducers, reconnect tests, and a rendered transcript assertion. |
| Migration work overlaps active session-state cleanup. | Treat `optimization/prd-session-state-consolidation` as an input; check its status before touching `AppState` or `commands/agent.rs`. |

## Rollout

Each PRD ships behind an internal read path or feature gate until its
compatibility tests pass. Migrate one existing project fixture and one live
project in a reversible pilot, then make the new read path default. Remove
legacy writes only after two consecutive releases can load, resume, and
inspect pre-migration data. The first PRD defines the exact rollback artifact
and gate; later PRDs must refer to it.
