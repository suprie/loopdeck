---
prd: prd-frontend-test-coverage
epic: optimization
milestone: "0.4.0"
status: proposed
description: >
  Stand up a frontend test toolchain (Vitest + Testing Library, matching the
  Vite + React 19 stack) and close the three coverage gaps a review flagged
  as highest-risk: streaming channels, migration cards, and permission-
  approval flows. Not a general coverage mandate — closes named gaps only.
order: 30
---

# PRD — Frontend Test Coverage

## Overview

`src-tauri` has 459 passing Rust tests. `src/` has none. The three areas
most likely to silently regress — streaming IPC channels, migration cards,
and permission-approval flows — are exactly the areas with the most
asynchronous, stateful UI logic and the least visual redundancy to catch a
regression by eye. This PRD adds a test toolchain and covers those three
areas.

## Problem Statement

- No frontend test command exists in `package.json` today, so there's
  nothing to run in CI or locally beyond `npx tsc --noEmit` (type-checking,
  not behavior).
- Streaming channels (agent output arriving incrementally over Tauri
  events) are the highest-complexity async surface in `src/hooks/` and have
  no test simulating event arrival order or partial-message handling.
- Migration cards (schema/data migration UI) run once per affected user and
  are easy to break without anyone noticing until a real migration fails.
- Permission-approval flows gate every mutating agent tool call — a UI bug
  here either blocks legitimate work or silently auto-approves something it
  shouldn't.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Add a frontend test toolchain (Vitest + React Testing Library) with a working `npm test` command and one trivial passing test, wired into whatever CI exists. |
| P0 | Cover streaming-channel behavior: incremental message arrival, out-of-order/partial chunks, and channel teardown. |
| P0 | Cover migration-card behavior: rendering the correct card for a given migration state, and the confirm/dismiss action paths. |
| P0 | Cover permission-approval flow: rendering the approval card for a control_request, approve/deny action paths, and the mode-aware badge states. |
| P1 | Document the test command in CLAUDE.md's "Development Commands" section. |

## Non-Goals

- 100% frontend coverage, or coverage thresholds enforced in CI — this PRD
  closes three named gaps, not a general mandate.
- End-to-end/browser automation (Playwright or similar) — component/unit
  level tests only, matching the toolchain's scope.
- Testing `Zustand` store logic beyond what the three flagged flows
  exercise incidentally.

## Design

_Stub — exact test file locations and mocking strategy for Tauri's
`invoke`/event APIs are a Phase 1 output, once the toolchain is actually
wired up against this codebase's IPC wrapper layer (`lib/tauri.ts`)._

## Phases

### Phase 1 — Toolchain setup

- [ ] Add Vitest + React Testing Library (+ jsdom) as dev dependencies;
      wire a `test` script in `package.json`.
- [ ] Establish a mocking pattern for `lib/tauri.ts`'s typed IPC wrappers
      (so component tests don't need a real Tauri runtime), with one
      trivial component test proving the pattern works.

### Phase 2 — Streaming channels

- [ ] Test incremental message arrival and correct render ordering for the
      streaming-channel hook(s) in `src/hooks/`.
- [ ] Test partial/out-of-order chunk handling and channel teardown
      (unmount mid-stream doesn't leak or crash).

### Phase 3 — Migration cards

- [ ] Test that the correct migration card renders for each migration
      state the component handles.
- [ ] Test confirm and dismiss action paths, including the IPC calls they
      trigger (via the Phase 1 mock).

### Phase 4 — Permission-approval flow

- [ ] Test the approval card renders correctly for a `control_request` and
      that approve/deny trigger the correct IPC calls.
- [ ] Test the mode-aware permission badge renders the correct state per
      `PermissionMode`/autonomous flag.

### Phase 5 — Docs

- [ ] Add the `npm test` command to CLAUDE.md's "Development Commands"
      section.

## Open Questions

- Is there an existing CI workflow to wire `npm test` into, or does this
  PRD also need to touch CI config? Check during Phase 1.
