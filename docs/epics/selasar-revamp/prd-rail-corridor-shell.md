---
prd: prd-rail-corridor-shell
epic: selasar-revamp
milestone: "0.5.0"
status: proposed
description: >
  Replace the feature-first sidebar (`AppShell.tsx`) with a 72px project
  rail — one door per registered project, glowing by that project's live
  run state, pinned-only past 5 projects — and replace the Dashboard/
  ProjectList grid with a corridor of room-card rows. Both render against
  `prd-rebrand-tokens`'s new palette.
---

# PRD — Rail & Corridor Shell

## Overview

Second PRD in the epic: the project-first navigation shell. Today's sidebar
is feature-first (Overview / Activity / Agent Runner / Decisions / Loops /
Epics as global pages); this PRD replaces it with a narrow rail of project
doors, and replaces the Dashboard's project grid with a corridor of wide
room-card rows. This PRD does not yet make clicking a door/card open the new
overlay drawer — `prd-detail-drawer` does that; here, a click can still land
on the existing routed `ProjectDetail` page as an interim target.

## Problem Statement

- `AppShell.tsx`'s sidebar organizes navigation by app feature, requiring a
  project to be picked separately (via the Dashboard) before any per-project
  view is reachable. The mockup's rail organizes navigation by project
  directly — no separate picker step.
- The mockup's rail only ever shows 6 example projects and defines no
  behavior once the project count grows past what a fixed-height icon
  column can hold — this needs its own answer, not silently borrowed from
  the mockup.
- `Dashboard.tsx` / `ProjectList.tsx` already carry most of the data a room
  card needs (name, path, description, `run_state`, progress); the
  corridor's job is presenting that data in the mockup's card shape, not
  sourcing new data.

## Goals

| Priority | Goal |
|----------|------|
| P0 | A 72px rail renders one door per registered project with a glow color reflecting that project's `RunState`, replacing the feature-first sidebar. |
| P0 | Past 5 registered projects, the rail shows pinned projects only, plus a fixed overflow entry back to the corridor — the rail never grows unbounded. |
| P0 | A corridor view renders one room-card row per project (name, path, description, status line, progress/phase-tick strip, last commit + dirty badge), replacing the current Dashboard/ProjectList grid. |
| P1 | Existing filter chips (All/Active/Archived) and the "Scan for repos" action carry over into the corridor toolbar. |
| P2 | `AttentionPanel`/`TodayPanel` either fold into room-card status lines or are explicitly kept as a separate corridor section — decided during Phase 2, not assumed. |

## Non-Goals

- Making the rail/corridor open the new overlay drawer — that's
  `prd-detail-drawer`; this PRD's click targets can remain the existing
  routed `ProjectDetail` page.
- The pin/unpin *mechanism*'s exact trigger UI is designed in this PRD
  (Phase 1), but any pin-state persistence beyond the current session is
  scoped to whatever's simplest — no new backend pin-storage design effort
  beyond a straightforward field.
- Night-run visuals on the rail/corridor beyond a basic indicator — the
  full night-run drawer experience is `prd-night-run-surfaces`.

## Design

Phase 1 decisions resolved during implementation (2026-08-12): pin/unpin is a
right-click context menu on the door itself (no room card exists yet this
phase); an existing install crossing 5 projects with zero pins shows the 5
most recently active as a fallback rather than an empty rail; pin state is a
plain `bool` on the global registry's `ProjectEntry` (per-machine, not
synced). The night-run indicator is a placeholder — derived from whether the
project has an active/queued `RunPlan` (existing run-queue data), pending
`prd-detail-drawer`'s spike settling the real `RunState` representation.

## Phases

### Phase 1 — Project rail

- [x] Build a `Rail` component (72px icon strip, one door per project, 2-letter
      initials) replacing `AppShell.tsx`'s current sidebar markup.
- [x] Wire each door's glow color to that project's `RunState`
      (working/waiting/done/idle) plus a distinct night-run indicator,
      reusing the state derivation already in `AttentionPanel.tsx` /
      `useAttentionItems`.
- [x] Add a pin/unpin affordance per project, and rail logic that shows all
      projects at 5 or fewer, or pinned-only plus one overflow door (back to
      the corridor) past 5.
- [x] Move the settings entry point to a gear icon at the rail's foot,
      preserving the existing `/settings` route.

### Phase 2 — Corridor room-card list

- [ ] Build a `RoomCard` component (name, path, description, status line +
      progress bar or phase-tick strip, last commit + relative time + dirty
      badge) replacing `ProjectList.tsx`'s row rendering, reusing its
      existing per-project data shape.
- [ ] Rebuild `Dashboard.tsx`'s page head/toolbar (title, filter chips,
      "Scan for repos") around the corridor layout, carrying over the
      existing `scanFolder`/`loadProjects` wiring.
- [ ] Decide and implement whether `AttentionPanel.tsx` and `TodayPanel.tsx`
      fold into room-card status lines/badges or remain a separate corridor
      section.

### Phase 3 — Verification

- [ ] Manual smoke test: rail-door click and corridor-card click both
      navigate to the same project's (still-routed) detail page.
- [ ] Manual smoke test: pin/unpin a project and confirm rail overflow
      behavior at 6+ registered projects.
- [ ] `npx tsc --noEmit` clean; visual pass against `prd-rebrand-tokens`'s
      light/dark tokens.

## Open Questions

- Where does the pin/unpin control live — a star on the room card, a
  right-click context menu, or an action inside the (not-yet-built) detail
  drawer header? Resolve in Phase 1 before building the affordance.
- For an existing install that already has more than 5 projects and no pins
  set yet, what does the rail show on first load after this ships — the 5
  most recently active, or an empty pinned rail with just the overflow
  door? Resolve in Phase 1.
- Is pin state per-machine (stored in `~/.config/loopdeck/config.yaml`) or
  something else? Default to the global config registry unless Phase 1
  finds a reason not to.
