---
prd: prd-session-state-consolidation
epic: optimization
milestone: "0.4.0"
status: proposed
description: >
  Consolidate AppState's five parallel Mutex<HashMap> pending-slot maps
  (lib.rs:107-113) into a single SessionState struct, and right-size
  commands/agent.rs (1731 lines) if it's still oversized once the maps are
  unified. The most concurrency-sensitive surface in the app, so this opens
  with a spike phase before any code moves.
---

# PRD — Session State Consolidation

## Overview

`AppState` (`lib.rs:107-113`) tracks agent/session lifecycle across five
separate `Mutex<HashMap>` fields instead of one. `commands/agent.rs` has
grown to 1731 lines around this sprawl. This is the app's most
concurrency-sensitive surface — the surface most likely to hide a race or a
lock-ordering bug — and it's also the least consolidated. This PRD unifies
the maps into one `SessionState` struct and right-sizes `agent.rs` if it's
still too large afterward.

## Problem Statement

Five independent lock-protected maps for what is conceptually one thing
(session state) means:

- Five separate lock acquisitions for operations that touch more than one
  map, with no single source of truth for lock ordering — a latent deadlock
  risk that only manifests under concurrent agent runs.
- New session-related state gets added as a sixth map rather than a field
  on an existing struct, because there's no single struct to extend.
- `commands/agent.rs` at 1731 lines mixes IPC handler logic with session
  bookkeeping across all five maps, making it hard to reason about which
  code touches which lock.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Map every current call site, lock-acquisition order, and cross-map invariant for the five existing `Mutex<HashMap>` fields before writing any new code. |
| P0 | Replace the five fields with one `SessionState` struct (single lock, or explicitly justified per-field locks if a spike shows contention requires it). |
| P1 | Migrate all `commands/agent.rs` (and any other) call sites to the new struct with no behavior change. |
| P1 | Right-size `commands/agent.rs` if it remains oversized once session bookkeeping is unified — split by responsibility, not by line count alone. |
| P2 | Add concurrency-focused tests for session-slot lifecycle (create/lookup/remove under concurrent access) that the old five-map shape had no reason to test in isolation. |

## Non-Goals

- Changing what session state is tracked — this is a consolidation of
  structure, not a change in semantics or a new feature.
- Removing the destructive-floor or permission-policy logic that happens to
  live near this code — out of scope, untouched.
- A general refactor of `commands/agent.rs` beyond what the consolidation
  requires — if the file is a reasonable size after the maps are unified,
  leave it.

## Design

_Stub — the shape of `SessionState` (one lock vs. per-field locks, exact
field names) is an output of the Phase 1 spike, not a decision made before
the current call sites and lock-ordering assumptions are actually mapped._

## Phases

### Phase 1 — Spike: map the current shape

- [ ] Enumerate every field in the five current `Mutex<HashMap>` maps in
      `AppState` (`lib.rs:107-113`): key type, value type, and every read/
      write call site across `commands/agent.rs` and elsewhere.
- [ ] Document current lock-acquisition order for any operation that
      touches more than one of the five maps, and flag any place two maps
      are locked in inconsistent order (deadlock risk).
- [ ] Propose the `SessionState` struct shape (fields, one lock vs.
      per-field locks) based on the above, with a one-paragraph rationale.

### Phase 2 — Implement SessionState

- [ ] Add the new `SessionState` struct and wire it into `AppState` in
      `lib.rs`, alongside (not yet replacing) the five old maps.
- [ ] Port one map's worth of call sites to the new struct as a proof of
      shape; confirm `cargo test` stays green.

### Phase 3 — Migrate and remove old maps

- [ ] Port the remaining call sites in `commands/agent.rs` (and any other
      file) from the five old maps to `SessionState`.
- [ ] Remove the five old `Mutex<HashMap>` fields from `AppState` once no
      call sites reference them.
- [ ] Split or reorganize `commands/agent.rs` if it remains oversized after
      the above, by responsibility (not an arbitrary line-count cut).

### Phase 4 — Concurrency tests

- [ ] Add tests exercising concurrent create/lookup/remove on
      `SessionState` slots, matching the lock-ordering invariants
      documented in Phase 1.
- [ ] Confirm the full `cargo test` suite (459+ tests) passes unchanged.

## Open Questions

- Does contention data from Phase 1 justify per-field locks inside
  `SessionState`, or is a single lock sufficient given actual call
  frequency? Resolve in Phase 1, not before.
