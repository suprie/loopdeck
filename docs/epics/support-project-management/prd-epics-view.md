---
prd: prd-epics-view
epic: support-project-management
milestone: "0.2.0"
status: proposed
description: >
  Surface the spec layer in the UI grouped by milestone, and build the single
  bridge action that connects a planned PRD checklist item to .loopdeck/loops.md
  execution. Cross-project /epics view + ProjectDetail Epics tab + promote-to-loop.
---

# PRD — Epics View + Promote-to-Loop Bridge

## Overview

Surface the spec layer in the UI and build the single bridge action that
connects a planned PRD checklist item to `.loopdeck/loops.md` execution. This
is where the hierarchy becomes usable: a human reads their epic, picks the
next loop, and promotes it — without the agent ever knowing epics exist.

Two surfaces:

1. A cross-project `/epics` view (sibling to `DecisionsView` / `LoopsView`)
   aggregating every project's epics.
2. An **Epics** tab in `ProjectDetail`, scoped to the open project.

And one action: **Promote to current loop**, which writes a PRD checklist item
into `loops.md ## Current` with its epic/prd back-reference.

## Problem Statement

The spec layer from `prd-spec-layer.md` is invisible without a UI. And
parseable epics alone don't drive execution — there's no path from "a planned
loop in a PRD" to "the current loop the agent runs." Today the human edits
`loops.md ## Current` by hand, which is exactly the friction that makes the
flat-list model painful past ~10 items.

## Goals

| Priority | Goal |
|----------|------|
| P0 | `/epics` route — epic cards grouped by **milestone** (from frontmatter), then by project within a milestone |
| P0 | Milestone groups are collapsible sections; milestone-less epics fall into an "Unmilestoned" group |
| P0 | Epic card: title, status badge, milestone, goal preview, PRD count, progress bar (done/total loops across PRDs) |
| P0 | Epics tab in `ProjectDetail` showing the open project's epics + PRD phase checklists |
| P0 | **Promote to current loop** action on any unchecked PRD checklist item |
| P0 | Promote writes the back-reference (`**Epic**`/`**PRD**`) into `loops.md ## Current` |
| P1 | Promote refuses to clobber a non-empty `## Current` — must complete/abandon first |
| P1 | Epics view shows `loops.md` History entries tagged with an epic/prd as "done" under their PRD |
| P2 | "Mark done" action on a PRD checklist item (manual sync — does not auto-derive from History) |

## Non-Goals

- Auto-syncing PRD checkboxes from `loops.md` History completion — manual in 0.2.0.
- Editing epics/PRDs in the app — they're authored in the editor, the app reads them.
- Promoting across projects (a PRD in project A cannot target project B's loops.md).
- Phase reordering or drag-and-drop — read-only rendering in 0.2.0.

## The Promote Contract

This is the load-bearing behavior; everything else is rendering.

**Input:** a project path, an epic slug, a PRD filename, and the checklist
item text (the planned loop title).

**Preconditions:**
- `loops.md ## Current` must be empty (no `Status: in_progress`). If non-empty,
  return `AppError::Conflict` with a message directing the user to complete or
  abandon the current loop. **Never clobber in-progress work.**

**Effect:** write into `loops.md`:

```markdown
## Current

- **Started**: <today>
- **Goal**: <the checklist item text>
- **Status**: in_progress
- **Epic**: <slug>
- **PRD**: <prd-filename-without-.md>
```

This reuses the existing `## Current` shape that `build_next_loop_prompt` and
`read_current_loop` already read — they don't need to know about the new
`**Epic**`/`**PRD**` fields (they ignore unknown fields). The agent's prompt
is built from `**Goal**` exactly as today.

**Postcondition:** the PRD checklist item is *not* auto-checked. The human
marks it done after the loop completes and lands in History.

## Phases

### Phase 1 — Cross-project `/epics` view, grouped by milestone

- [x] `epics-view/cross-project-view` `EpicsView.tsx` — load all projects' epics via `getEpicsByMilestone`, render milestone sections (collapsible)
- [x] `epics-view/milestone-section` Milestone section: header with milestone label + epic count; epics grouped by project within the section
- [x] `epics-view/epic-card` Epic card: title, status badge, milestone, goal preview, PRD count, progress bar (done/total loops across PRDs)
- [x] `epics-view/expand-in-place` Expand-in-place: click epic → show PRD list; click PRD → show phase checklists
- [x] `epics-view/unmilestoned-group` Unmilestoned group for epics whose frontmatter omits `milestone`
- [x] `epics-view/empty-states` Empty / loading / error states (mirror `DecisionsView.tsx` patterns)
- [x] `epics-view/route-and-nav` Add `/epics` route in `router.tsx` + nav item in `AppShell.tsx`

### Phase 2 — ProjectDetail Epics tab

- [x] `epics-view/project-detail-tab` Add `epics` tab to `ProjectDetail.tsx` (sibling to Overview / Decisions / Loops / Agent)
- [x] `epics-view/promote-action` Render the open project's epics + PRD phase checklists with Promote action on each unchecked item
- [x] `epics-view/current-loop-highlight` Show which PRD's loop is currently in `loops.md ## Current` (highlight + disable other Promote buttons while one is active)

### Phase 3 — Promote-to-loop bridge (backend)

- [ ] `epics-view/promote-command` `promote_epic_loop(project_path, epic_slug, prd_filename, loop_title) -> Result<()>` Tauri command
- [ ] `epics-view/clobber-guard` Clobber guard: refuse if `loops.md ## Current` is non-empty (`parse_loops` status check)
- [ ] `epics-view/current-backref` Write `## Current` block with `**Epic**`/`**PRD**` back-reference, preserving the rest of `loops.md`
- [ ] `epics-view/promote-tests` Unit tests: empty current → success; non-empty current → `Conflict`; back-reference fields present

### Phase 4 — Done-visibility (read path)

- [ ] `epics-view/done-in-history` `get_epics` enriches each PRD checklist item with `done_in_history: bool` by scanning `loops.md ## History` for entries whose `**Epic**`/`**PRD**` match and whose title matches the item
- [ ] `epics-view/render-done` Render done items as checked in the UI (read-only sync from History → PRD display)
- [ ] `epics-view/manual-mark-done` Manual "Mark done" action for cases where the title drifted (writes the check into the PRD file)

## Open Questions

- Title-drift: if a promoted loop's title gets edited in `loops.md` before
  completion, the History-match for done-visibility fails. **Lean:** accept
  the miss, surface it as "unmatched done loop" in the UI rather than guessing.
  The human marks it done manually. Don't build fuzzy matching.
- Should the Epics tab live in `ProjectDetail` or replace the Loops tab?
  **Lean:** separate tab. Loops is the execution log; Epics is the plan.
  Overloading Loops recreates the plan+execution-mixed-in-one-file problem
  that motivated the split.
