---
prd: prd-assign-loop-id
epic: overnight-orchestration
milestone: "0.4.0"
status: proposed
description: >
  A one-click action that generates a stable `epic/slug` ID for a loop that
  doesn't have one and writes it into its PRD checklist line, so an existing
  loop authored before the stable-ID convention (or added by hand without one)
  can be queued for an overnight run without a manual markdown edit.
---

# PRD — Assign Loop ID

## Overview

`prd-run-queue`'s picker (`EpicsPanel.tsx`) already disables the overnight-run
checkbox for any loop with no stable `id` — correctly, since `execution_id` is
the join key `execute_run` needs to find the loop again (`epic::find_loop_by_id`)
and PR #63's combined-turn batching consumes exactly the same key. That
restriction is not the gap. The gap is that assigning an id has no supported
action: today it means opening the PRD file and hand-typing a `` `epic/slug` ``
prefix onto the checklist line. This PRD closes that one gap with a small,
targeted action — it does not touch the queue/combine pipeline at all.

## Problem Statement

`PrdLoop.id: Option<String>` (`epic.rs`) is parsed from an optional
`` `namespace/loop` `` backtick prefix on a checklist line — set by hand, or by
`loopdeck:epic-author` when a loop is freshly drafted. Loops written before the
stable-ID convention landed (`prd-structured-execution-state`, 0.2.1), or added
by hand since without following the convention, have no id. `EpicsPanel.tsx`'s
picker checkbox is disabled for them (`disabled={noId}`) with a tooltip
explaining why — correct behavior, but a dead end: there is no button, command,
or flow that gets that loop an id. The only path today is manually editing the
PRD markdown file outside the app.

## Goals

| Priority | Goal |
|----------|------|
| P0 | New IPC command that generates a collision-free stable id for a named loop and rewrites its checklist line in the PRD file in place |
| P0 | "Assign ID" action in `EpicsPanel.tsx`'s picker row for id-less, not-done loops, calling the new command and refreshing epics state |
| P0 | Generated ids pass `epic::validate_loop_ids` with no new diagnostics |
| P1 | Assigning an id immediately enables that loop's picker checkbox without a manual reload |

## Non-Goals

- Bulk/batch id assignment across a whole PRD or epic in one action — one loop
  at a time, matching the picker's existing per-row interaction model.
- Editing or renaming an id a loop already has — assignment only, id-less
  loops only.
- Any change to `execute_run`, `RunPlan`, `next_queued_batch`, or the combined-
  turn prompt building PR #63 shipped — this PRD only gets more loops into that
  existing, unmodified pipeline.
- Auto-assigning ids at draft time — `loopdeck:epic-author` already does this
  for loops it drafts fresh; this PRD targets loops that exist without one
  today.

## Design

Directional; refine during implementation.

- **ID generation**: kebab-case slug derived from the loop's title, scoped
  under the loop's own epic slug (`epic-slug/title-slug`), matching the shape
  every existing hand-authored id already uses. Collision-checked against
  every id already parsed in that epic (reuse `epic::validate_loop_ids`'s
  parse path, or the equivalent loop-collection code it shares); on collision,
  append a numeric suffix (`-2`, `-3`, ...).
- **Markdown rewrite**: locate the exact checklist line by its current
  (id-less) text — the caller already has epic/prd/phase/line-index from the
  same read that renders the picker — and rewrite only that line, preserving
  its checked state (`- [x]`/`- [ ]`) and surrounding file content untouched.
  Line-targeted string replacement, not a structured markdown parser rewrite
  of the whole file — the existing `epic.rs` parser is read-only today and
  this is the first write path into a PRD checklist line's *text*, as
  distinct from `toggle_prd_loop`'s existing checked-state-only write.
- **Frontend**: an "Assign ID" affordance next to the disabled picker checkbox
  for `noId` loops in `EpicsPanel.tsx` (same `!done` guard the picker checkbox
  and promote button already use), calling the new `assign_loop_id` IPC
  command and refreshing `epics` state on success so the checkbox unlocks
  immediately.

## Phases

### Phase 1 — Backend: ID generation + markdown rewrite

- [x] `assign-loop-id/generate-collision-free-slug` Pure `generate_loop_id(epic_slug, title, existing_ids) -> String` helper: kebab-case from title, numeric-suffix on collision
- [x] `assign-loop-id/rewrite-checklist-line` `assign_loop_id` IPC command: locate the target checklist line (epic/prd/phase/loop identity from the caller), rewrite only that line with the generated id prefix, preserve checked state and every other line byte-for-byte
- [x] `assign-loop-id/reject-already-id-loops` Command rejects (or no-ops) when the target loop already has an id — assignment only, never overwrite

### Phase 2 — Frontend: picker action

- [x] `assign-loop-id/picker-action` "Assign ID" action next to the disabled picker checkbox for `noId`, not-done loops in `EpicsPanel.tsx`
- [x] `assign-loop-id/refresh-on-success` Successful assignment refreshes epics state and the picker checkbox enables without a manual reload

### Phase 3 — Tests

- [ ] `assign-loop-id/collision-tests` Rust tests: slug generation collision handling (0, 1, 2+ collisions), rejection on an already-id'd loop
- [ ] `assign-loop-id/round-trip-tests` Rust tests: markdown rewrite round-trips against this repo's own real PRD files — every other line and the target line's checked state are byte-for-byte unchanged, only the id prefix is added
- [ ] `assign-loop-id/frontend-check` `npx tsc --noEmit` green; picker action wired end-to-end against a fixture epic with an id-less loop

## Open Questions

- Id scope: `epic-slug/title-slug` (matching every existing hand-authored id)
  or should it also fold in the PRD/phase for extra collision safety? Bias:
  keep it the same shape as existing ids — a longer scheme diverges from the
  convention `epic-author` already establishes.
- Should the picker row show the generated id before or only after writing it
  (a "preview, then confirm" step vs. one click assigns immediately)? Bias:
  one click — the id is cosmetic/internal, not something the user needs to
  approve wording on, unlike a PR body.
