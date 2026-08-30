---
prd: prd-detail-drawer
epic: selasar-revamp
milestone: "0.5.0"
status: proposed
description: >
  Replace the routed, full-page `ProjectDetail` (`/project/$projectPath`)
  with a right-side overlay slide-over (Overview/Agent/Loops/Decisions
  tabs), after a spike phase settles the routing model and where the
  current Epics and Graph tabs relocate.
---

# PRD — Detail Drawer

## Overview

Third PRD, and the epic's biggest structural risk: converting a routed,
6-tab, full-page view into a 4-tab overlay that opens from the rail/corridor
built in `prd-rail-corridor-shell`. Because collapsing a route into an
overlay can silently drop functionality (deep-linking, browser-back, two
whole tabs), this PRD opens with a spike phase whose only output is a
decision, recorded as an ADR, before any drawer UI is built.

## Problem Statement

- `ProjectDetail.tsx` is currently a real route (`/project/$projectPath`)
  with 6 tabs: overview, agent, decisions, loops, epics, graph. The mockup's
  drawer is a non-routed overlay (plain show/hide state, no URL change) with
  only 4 tabs.
- Losing route-backing means losing bookmarkability and browser-back
  behavior for a project's detail view, unless the drawer state is
  represented in the URL some other way (query param, hash) — undecided.
- The mockup doesn't have an Epics or Graph tab at all; the current app's
  `EpicsPanel.tsx` (which already hosts `RunQueuePanel`, the night-run data
  source for `prd-night-run-surfaces`) and `KnowledgeGraphPanel.tsx` need an
  explicit new home, not a silent drop.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Decide and record (as an epic ADR) whether the drawer stays URL-backed or is pure UI state, and where the Epics and Graph tabs relocate. |
| P0 | Build the drawer's standard variant (scrim, slide-over panel, header, Overview/Agent/Loops/Decisions tab rail), reusing existing tab content components. |
| P0 | Wire rail-door and corridor-card clicks to open the drawer instead of navigating to the old full-page route. |
| P1 | Relocate Epics and Graph tab content per the spike's decision, with no loss of existing functionality (starting loops, viewing the knowledge graph). |

## Non-Goals

- Building the night-run variant of the drawer, the Plan-tonight wizard, or
  the morning report — all `prd-night-run-surfaces`, which depends on this
  PRD's drawer shell existing first.
- Redesigning the content *within* Overview/Agent/Loops/Decisions beyond
  what fitting them into a narrower drawer requires — this PRD relocates and
  restyles, it doesn't redesign each tab's internal UX.
- Changing what `RunState` values exist — that question is explicitly
  deferred to whichever PRD ends up needing it (likely
  `prd-night-run-surfaces`), this PRD's spike only needs to know the answer
  will exist by the time the night variant is built.

## Design

_Stub — the spike phase (Phase 1) is this PRD's actual design work; nothing
below it should be built until the spike's ADR is recorded._

## Phases

### Phase 1 — Spike: drawer routing model + tab relocation

- [x] `selasar-revamp/decide-whether-the-drawer-stays-url-backed-route-or-query-param
` Decide whether the drawer stays URL-backed (route or query-param
      change, rendered as an overlay instead of a page swap) or is pure UI
      state with no routing; weigh deep-link/bookmark/back-button behavior
      against the mockup's plain show/hide implementation. Record the
      decision as an ADR in the epic README. — ADR-1: pure UI state, see
      `README.md`.
- [x] Decide where the Epics tab (`EpicsPanel.tsx`, including
      `RunQueuePanel`) and Graph tab (`KnowledgeGraphPanel.tsx`) relocate —
      folded into one of the four remaining tabs, added as a fifth tab
      beyond the mockup's four, or reachable another way. Record the
      decision as an ADR. — ADR-2: nested sub-tabs under Loops/Decisions,
      see `README.md`.
- [x] Confirm with whoever owns `prd-night-run-surfaces` sequencing whether
      "night run" needs to exist as a real `RunState` variant by the time
      this drawer's variant-selection logic is built, so that logic isn't
      built twice. — ADR-3: not this spike's blocker, `prd-night-run-surfaces`
      already owns that question in its own Phase 1.

### Phase 2 — Overlay drawer shell

- [x] Build the drawer's standard variant (scrim + slide-over panel, header
      with path/name/description, Overview/Agent/Loops/Decisions tab rail)
      per the spike's routing decision, reusing existing tab content
      components (`OverviewTab`, `AgentPanel`, `LoopsPanel`,
      `DecisionsPanel`). — `ProjectDrawer.tsx`, built on shadcn `Sheet`.
- [x] Wire rail-door clicks (from `prd-rail-corridor-shell`) and
      corridor-card clicks to open the drawer instead of navigating to the
      old full-page route. — `Rail.tsx`, `Dashboard.tsx`, `AttentionPanel.tsx`,
      `CommandPalette.tsx`, `useStuckSessions.ts` all switched to
      `appStore.openDrawer()`.
- [x] Relocate Epics and Graph tab content per the spike's decision. —
      `LoopsTabContent`/`DecisionsTabContent` in `ProjectDrawer.tsx`.

### Phase 3 — Verification

- [ ] `selasar-revamp/manual-smoke-test-open-close-the-drawer-from-both-rail-and-corridor` Manual smoke test: open/close the drawer from both rail and corridor,
      tab switching, and — if URL-backed — direct-URL load and
      browser-back behavior. — Deferred to human; this is a Tauri desktop
      app with no browser-mode IPC mock in this repo (same documented gap
      as every prior UI-only loop in `loops.md`).
- [x] `npx tsc --noEmit` clean; confirm no regression in existing per-tab
      functionality (Agent chat, starting a loop, decisions list, epics
      list, knowledge graph) now rendered inside the drawer.

## Open Questions

- If the drawer becomes pure UI state (no routing), how does a user share
  or return to "this specific project's view" — is that simply out of
  scope, or does something else (e.g. a last-opened-project preference)
  cover it? Resolve in Phase 1.
- Does the drawer need to support being open while the user also navigates
  the corridor behind it (e.g. scrolling), or does opening it always freeze
  the corridor underneath? Resolve during Phase 2 build.
