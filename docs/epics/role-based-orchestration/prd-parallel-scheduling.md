---
prd: prd-parallel-scheduling
epic: role-based-orchestration
milestone: "0.6.0"
status: proposed
description: >
  Execute run plans one phase per turn and run phases with satisfied,
  disjoint depends_on concurrently in separate worktrees, with gated starts
  for dependents, multi-in-flight restart reconciliation, and per-phase
  budgets — ending the combined-batch single LLM turn.
---

## Overview

The run queue currently combines every queued phase into one LLM turn
(`next_queued_batch`, `commands/run_queue.rs:850`) and `depends_on` edges
matter only for stall-parking, not sequencing. This PRD makes phases the
unit of execution: one turn per phase, concurrent turns where the
dependency graph allows, and Waiting gates for dependents. Overnight-
orchestration ADR-3 deferred this on purpose — worktrees were built so
parallel is "an upgrade, not a rewrite".

## Problem Statement

Role-staffed plans need different agents working at the same time (dev and
QA on independent phases; two devs on disjoint phases), and a single
combined turn cannot be attributed to roles, budgeted per phase, or
arbitrated. Sequential-only execution caps the orchestration epic at
theater.

## Goals

| Priority | Goal |
|---|---|
| P0 | One executor turn per run-plan phase, replacing the combined batch |
| P0 | Concurrent execution of phases whose `depends_on` are satisfied and disjoint, each in its own worktree, under a bounded concurrency limit |
| P0 | Gated starts: dependent phases hold in Waiting until upstream completes, then release or fail per the stall policy |
| P1 | Restart reconciliation covering multiple in-flight phases |
| P1 | Per-phase token and wall-clock budgets per in-flight phase, not per batch |

## Non-Goals

- Cron, delays, or recurring schedules (deferred epic-wide)
- Cross-project scheduling or global budgets
- Changing the one-run-per-project executor lock (within a run, phases parallelize)
- Auto-merging concurrent branches (arbitration is prd-arbitration)

## Design

Stub — points to resolve while implementing:

- Scheduling primitive: topological batches versus a ready-set with a
  semaphore; must reject cycles at plan creation
- Session cache safety: sessions are keyed by worktree path
  (`AppState.claude_sessions`), so one worktree per in-flight phase keeps
  the existing keying honest — confirm against multi-agent runs coexisting
- Budget watchdog per phase reusing the existing kill mechanics rather
  than new timeout machinery

## Phases

### Phase 1 — Per-phase turn execution

- [ ] `parallel-scheduling/one-turn-per-phase` Replace the combined queued-batch turn with one executor turn per run-plan phase

### Phase 2 — Dependency-aware fan-out

- [ ] `parallel-scheduling/parallel-batches` Run phases with satisfied, disjoint depends_on concurrently in separate worktrees under a bounded concurrency limit
- [ ] `parallel-scheduling/gated-start` Hold dependent phases in Waiting until upstream phases complete, releasing or failing per the stall policy

### Phase 3 — Reconciliation + budgets

- [ ] `parallel-scheduling/restart-reconcile` Extend restart reconciliation to multiple in-flight phases after a crash or restart
- [ ] `parallel-scheduling/budget-split` Apply per-phase token and wall-clock budgets per in-flight phase instead of per combined batch

### Phase 4 — Tests/verification

- [ ] `parallel-scheduling/tests` Add scheduler property tests (diamond graphs, cycle rejection, mid-flight crash recovery) and worktree cleanup race checks

## Open Questions

- Default concurrency limit, and is it user-configurable per run plan?
- When a phase fails, do downstream Waiting phases fail fast or attempt independent continuation per the stall policy?
- Do multi-agent comparison runs and parallel phases coexist in one plan, or remain mutually exclusive per phase?
