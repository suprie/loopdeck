---
prd: prd-operational-store
epic: execution-foundations
milestone: "unassigned"
status: proposed
description: >
  Persist authoritative mutable run and decision state locally, migrate old
  records safely, and reconcile interrupted work on restart.
order: 30
---

# PRD — Operational Store

## Overview

Implement the store selected by `prd-domain-contract`. SQLite is the proposed
default, subject to that PRD's evidence gate. This PRD owns mutable run state;
authored specs and decisions remain repo files. The migration must be
reversible until old data has been proven readable through the new path.

## Problem Statement

Current operational fields are distributed across YAML, JSONL, and in-memory
maps. Atomic file writes and restart reconciliation already protect parts of
that system, but there is no single transaction covering a run transition and
its related decision or evidence.

## Goals

| Priority | Goal |
|---|---|
| P0 | Implement schema-versioned project, run, phase, session, decision, and delivery-evidence records using the approved store. |
| P0 | Route run transitions through one transactional service with idempotent command keys and explicit conflict errors. |
| P0 | Import preexisting state without modifying source files; provide backup, restore, and rollback checks. |
| P1 | Reconcile stale running work on restart and expose one backend run snapshot API. |

## Non-Goals

- Migrate human-authored `docs/epics/` or `.loopdeck/decisions.md`.
- Change existing permission or delivery gates.
- Delete legacy files as part of first release of the store.

## Design

Keep repository paths as locators and store a stable project ID separately.
Preserve spec phase IDs as external references. The write service validates
the pure transition contract, updates state in one transaction, and returns a
revisioned snapshot. Legacy import records provenance and is repeatable.
Compatibility reads compare old and new projections during the pilot; a
visible diagnostic reports divergence instead of silently picking one.

## Phases

### Phase 1 — Store and import foundation

- [ ] `operational-store/create-schema` Add the approved versioned store schema, migration runner, integrity check, and backup/restore command or documented procedure.
- [ ] `operational-store/import-legacy` Import representative interactive, queued, multi-agent, and delivered fixtures with stable IDs and provenance; prove repeat import is idempotent.

### Phase 2 — Transactional transitions

- [ ] `operational-store/migrate-interactive` Route interactive runs through transactional transitions and idempotent command keys; preserve old reads during the pilot.
- [ ] `operational-store/migrate-queue` Route queued runs and phase status through the same service; compare the new projection with `run-plan.yaml` on pilot fixtures.
- [ ] `operational-store/migrate-multi-agent` Route multi-agent runs through the same service; compare the new projection with existing manifests on pilot fixtures.
- [ ] `operational-store/migrate-decisions` Persist pending decisions by stable ID and expose a switchable read path; report discrepancies instead of hiding them.
- [ ] `operational-store/reconcile-restart` Reconcile processes absent after restart to `interrupted` or the contract's terminal status, including parked decisions and retained worktrees.

### Phase 3 — Verification and rollout

- [ ] `operational-store/test-recovery` Test malformed/partial legacy data, interrupted writes, concurrent command conflicts, backup restore, and rollback against pre-migration fixtures.
- [ ] `operational-store/pilot-read-path` Pilot the new read path on one fixture and one real project, record comparison results, then enable it by default only when zero unexplained divergence remains.

## Acceptance Criteria

- A transition and its related decision/evidence update commit together or
  neither commits; retrying the same command does not duplicate the change.
- A restart never reports an absent process as live, and no archived session
  or delivery evidence disappears from the read model.
- Old authored files remain intact; a tested restore returns to the previous
  readable state without editing the repository's specs.
- The default read path changes only after the pilot comparison passes.

## Open Questions

- Which operational fields in `run-plan.yaml` must remain exported for other
  tools? Set the compatibility export contract in Phase 1.
