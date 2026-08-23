---
title: Selasar Revamp
slug: selasar-revamp
milestone: "0.5.0"
status: proposed
started: 2026-08-05
owner: Suprie
description: >
  A user reads every project's status — including unattended overnight runs —
  from a corridor of project doors, and manages each project (chat, loop
  history, decisions, night-run planning) from one slide-over panel, instead
  of a feature-first sidebar and separate full-page views. Ships alongside a
  rebrand of the app from LoopDeck to Selasar (skin and copy only — no
  `.loopdeck/` schema or config-path change).
---

# Epic — Selasar Revamp

## Motivation

A shared design mockup (Claude artifact `d43c98ab`, "Selasar — every project
you have, lives here") proposes a full information-architecture inversion of
the app shell: instead of a feature-first sidebar (Overview / Activity /
Agent Runner / Decisions / Loops / Epics as global pages, with a project
picked from a Dashboard list), navigation becomes project-first — a narrow
rail of one icon "door" per registered project, glowing by that project's
live run state, opening a slide-over drawer scoped to that project alone.

The 0.4.0 `overnight-orchestration` epic already shipped the backend this
depends on: `RunPlan`, `RunPhase`, `RunConsent`, `RunBudgets`, `PhaseVerdict`,
and `AuditSlice` (`src-tauri/src/runplan.rs`, `run_executor.rs`) are live, and
`RunQueuePanel.tsx` / `MultiAgentRuns.tsx` already render most of the data the
mockup's "Plan tonight" wizard and morning report need. This epic is
overwhelmingly a **frontend IA and visual revamp** — a new shell, a new
drawer, and a reskin of existing run-queue panels onto it — not new backend
capability.

The mockup also renames the app to "Selasar" throughout, including its own
window title and wordmark. That rebrand ships as part of this epic: naming
and copy only, with the on-disk `.loopdeck/` directory name and
`project.yaml` schema left untouched (matching the mockup's own recorded
design decision to avoid a migration for existing installs).

## Scope

In scope:

- **Design tokens + typography**: replace `src/styles.css`'s current
  Tailwind-v4 token set (Inter + JetBrains Mono, dark-first) with the
  mockup's paper/ink/teak palette (light default, full dark pair) and serif
  display font for wordmark/project names, keeping the existing
  light/dark/system theme toggle working against the new values.
- **Selasar rebrand**: window title, wordmark, rail mark, and user-facing
  copy across the app (Dashboard greeting, empty states, Settings, docs)
  updated to Selasar. No change to `.loopdeck/` directory name,
  `project.yaml` schema, or any persisted config/registry format.
- **Project rail**: a 72px icon strip replacing `AppShell.tsx`'s feature-first
  sidebar — one door per registered project, colored glow keyed to that
  project's `RunState` (working/waiting/done/idle) plus a night-run
  indicator, settings gear at the rail's foot. The mockup only shows 6
  projects and doesn't address scale: past 5 registered projects, the rail
  shows **pinned projects only** plus an overflow entry back to the full
  corridor, instead of growing an unbounded scrollable icon column.
- **Corridor list**: room-card rows replacing `Dashboard.tsx` /
  `ProjectList.tsx`'s current grid — name, path, description, status line,
  progress bar or phase-tick strip, last commit + relative time + dirty
  badge — with the existing filter-chip and scan-for-repos affordances
  carried over.
- **Detail drawer**: a right-side overlay slide-over (Overview / Agent /
  Loops / Decisions tabs) replacing the routed, full-page
  `ProjectDetail.tsx` (`/project/$projectPath`) — a spike phase resolves
  whether the drawer stays URL-backed and where the current Epics and Graph
  tabs relocate before the shell is built.
- **Night-run surfaces**: a night variant of the drawer (phase rail, budget
  gauges, inline parked-question requeue), a "Plan tonight" wizard (phase
  picker, stall policy, budgets, pre-flight interview, consent), and a
  morning-report drawer (verdict table, parked questions, kill callouts,
  audit-log tail) — all a reskin/relocation of `RunQueuePanel.tsx` and
  `MultiAgentRuns.tsx` onto the new drawer, not new backend work.
- **New home for global Activity + Epics views**: the current `/activity`
  and `/epics` top-level pages lose their sidebar slot under the
  project-first rail; this epic finds and builds their replacement surface
  (per-project tab, command-palette destination, or other) rather than
  dropping the functionality.

Out of scope (deferred or explicitly not doing):

- Any change to `.loopdeck/` on-disk schema, `project.yaml` format, or the
  config directory name — rebrand is skin/copy only.
- New backend budget, consent, or run-plan capabilities beyond what
  `overnight-orchestration` already shipped.
- Command-palette redesign, beyond adding targets for any page that
  relocates.
- Mobile or responsive layout — desktop-only app, matching the mockup.

## PRD Index

| PRD | Covers |
|-----|--------|
| [prd-rebrand-tokens.md](./prd-rebrand-tokens.md) | Paper/ink/teak design tokens + serif display font in `styles.css` (light/dark/system), Selasar rebrand copy (window title, wordmark, app strings) |
| [prd-rail-corridor-shell.md](./prd-rail-corridor-shell.md) | 72px project rail (door icons + `RunState` glow) replacing the feature-first sidebar; corridor room-card list replacing the Dashboard/ProjectList grid |
| [prd-detail-drawer.md](./prd-detail-drawer.md) | Spike phase (routed-vs-overlay decision, Epics/Graph relocation) + overlay slide-over drawer (Overview/Agent/Loops/Decisions) replacing routed `ProjectDetail` |
| [prd-night-run-surfaces.md](./prd-night-run-surfaces.md) | Night drawer variant, "Plan tonight" wizard, morning-report drawer — reskin of `RunQueuePanel`/`MultiAgentRuns` onto the new drawer |

**Delivery order is strict — index order, each PRD depends on artifacts of
the previous.** `prd-rebrand-tokens` gates everything visual (the rail and
drawer are themed against its tokens); `prd-rail-corridor-shell`'s rail and
cards are what the drawer opens from; `prd-detail-drawer`'s spike must land
before its own overlay build, and before `prd-night-run-surfaces` can reskin
a night variant onto a drawer that doesn't exist yet.

## Architecture Decisions

### ADR-1: Detail drawer is pure UI state, not URL/route-backed (2026-08-23)

- **Status**: accepted
- **Context**: `prd-detail-drawer` Phase 1 spike. The routed
  `/project/$projectPath` full-page view needed to become a slide-over
  overlay. Open question: keep it URL-backed (route or query param, rendered
  as an overlay) for bookmark/deep-link/back-button behavior, or make it
  plain show/hide state matching the mockup exactly.
- **Decision**: Pure UI state — `appStore.drawerOpen` (bool) +
  `selectedProjectPath` (already existed, decoupled from routing).
  `ProjectDrawer.tsx` mounts app-wide in `AppShell.tsx`, not inside a route.
  The old `/project/$projectPath` route and `ProjectDetail.tsx` are deleted.
- **Why**: This is a Tauri desktop app with an in-memory router and no
  visible address bar — the browser back-button and bookmarking value a real
  route buys a web app doesn't apply here. `selectedProjectPath` was already
  a plain store field, not derived from route params, so route-backing was
  already partly fictional before this change. The rail/corridor (this
  epic's actual navigation model) opens the drawer directly; nothing in the
  mockup or this app's usage pattern needs to survive a process restart or
  be shared as a link.
- **Consequences**: No deep-link or shareable-URL to a specific project's
  drawer state (open question from the PRD — explicitly out of scope, not
  revisited: nothing in this app currently generates or consumes such a
  link). Reopening the app always lands on the corridor/dashboard;
  `selectedProjectPath`/`drawerOpen` are session-only, matching every other
  transient UI-state field in `appStore.ts`. `CommandPalette`,
  `AttentionPanel`, `useStuckSessions`, and `useProjects` all switched from
  `navigate({ to: "/project/$projectPath" })` to `appStore.openDrawer(path)`.

### ADR-2: Epics and Graph tabs relocate as nested sub-tabs, not top-level (2026-08-23)

- **Status**: accepted
- **Context**: The mockup's drawer has 4 tabs (Overview/Agent/Loops/
  Decisions); the current app's routed view has 6 (adds Epics and Graph).
  Open question: fold Epics/Graph into one of the 4, add a 5th/6th tab
  beyond the mockup, or reach them another way.
- **Decision**: `EpicsPanel.tsx` (incl. `RunQueuePanel`) becomes a nested
  sub-tab under **Loops** (`LoopsTabContent` in `ProjectDrawer.tsx`, a
  `Loops | Epics` sub-tab pair); `KnowledgeGraphPanel.tsx` becomes a nested
  sub-tab under **Decisions** (`DecisionsTabContent`, a `Decisions | Graph`
  pair). Both panels are reused unchanged, not redesigned. The legacy
  `DetailTab` union keeps its `"epics"`/`"graph"` values (deep-selects the
  right sub-tab from callers like `useProjects.createProject`); a new
  `topLevelTab()` helper maps them onto the drawer's 4 top-level tabs.
- **Why**: Epics/loops and epics/graph are each already conceptually paired
  (an epic's loops *are* loop-queue items; the graph is a project-wide view
  most relevant next to its decision history) — nesting keeps the drawer at
  the mockup's literal 4-tab width instead of growing it, with zero
  functionality dropped.
- **Consequences**: None of `EpicsPanel`'s or `KnowledgeGraphPanel`'s own
  code changed — only where they're mounted from. `RunQueuePanel` (the
  night-run data source `prd-night-run-surfaces` needs) stays reachable at
  Loops → Epics.

### ADR-3: `prd-night-run-surfaces`'s `RunState` question is not this spike's blocker (2026-08-23)

- **Status**: accepted
- **Context**: Phase 1's third item asks to confirm with whoever owns
  `prd-night-run-surfaces` sequencing whether "night run" needs to be a real
  `RunState` variant by the time this drawer's variant-selection logic is
  built.
- **Decision**: No cross-PRD confirmation needed before this PRD's Phase 1/2
  land. This PRD's Non-Goals already exclude changing what `RunState` values
  exist, and `prd-night-run-surfaces.md`'s own Design section (line 37)
  already tracks "new enum value vs. derived flag" as *its own* Phase 1
  open question, resolved when that PRD's Phase 1 is built — not by this
  spike guessing ahead of it.
- **Why**: This PRD's Phase 2 build (the drawer shell) has no
  variant-selection logic at all — that branch point is introduced by
  `prd-night-run-surfaces` itself. Answering it here would be resolving a
  different PRD's open question without that PRD's own Phase 1 context.
- **Consequences**: `prd-night-run-surfaces` Phase 1 inherits this question
  unchanged; no action taken in this repo beyond this note.

## Success Criteria

- The project rail shows one icon per registered project with a glow color
  that reflects that project's live `run_state` (working/waiting/done/idle),
  plus a distinct indicator when an unattended overnight run is active.
- With 5 or fewer registered projects, the rail shows all of them; past that,
  it shows only pinned projects plus an "all projects" overflow entry that
  opens the corridor — the rail never grows past a fixed height regardless of
  how many projects are registered.
- The corridor replaces the current Dashboard/ProjectList grid with room-card
  rows (name, path, description, status line, progress/phase-tick strip,
  last commit + dirty badge), sorted by recent activity.
- Clicking a project (rail door or corridor card) opens a right-side overlay
  drawer with Overview/Agent/Loops/Decisions tabs — no full-page navigation —
  and an active night run swaps the Agent tab for a phase rail, budget
  gauges, and parked-question requeue.
- The "Plan tonight" wizard queues an unattended run (phase selection, stall
  policy, budgets, pre-answered questions, consent) against the existing
  `run_executor`/`runplan.rs` backend with no new backend endpoints; a
  morning-report drawer surfaces verdicts, parked questions, and kills once
  the run finishes.
- App identity reads "Selasar" everywhere user-visible (window title,
  wordmark, in-app copy), while `.loopdeck/` on-disk config path and schema
  are unchanged and existing installs need no migration.
- The functionality currently on the global Activity and Epics pages remains
  reachable somewhere in the revamped app — not silently dropped.

## Risks

| Risk | Mitigation |
|------|-----------|
| **Routed → overlay drawer loses functionality** — collapsing the deep-linkable `/project/$projectPath` route (6 tabs) into a non-routed overlay (4 tabs) drops bookmarking/back-button behavior and strands the Epics/Graph tabs with no home | Dedicated spike phase in `prd-detail-drawer`, before the overlay is built, explicitly decides the routing model and the Epics/Graph destination |
| Rebrand copy sprawl — "LoopDeck" is likely hardcoded in more places than the obvious wordmark (window config, error strings, docs, `about`/settings copy) and a partial rename reads worse than no rename | `prd-rebrand-tokens` includes a full-repo string audit for user-visible "LoopDeck" occurrences as an explicit phase, not just the components the mockup shows |
| Token swap regresses dark mode or contrast for a component the mockup didn't mock up (e.g. destructive/error states, focus rings) | `prd-rebrand-tokens` phase checklist includes a pass over existing components against the new tokens in both themes, not just the mockup's own surfaces |
| Night-run reskin drifts from what `run_executor.rs` actually emits (phase status enum, park payload shape, audit event format) since the mockup's data is fabricated example content | `prd-night-run-surfaces` phases start from reading the real `RunPlan`/`RunReport`/`AuditSlice` types and existing `RunQueuePanel.tsx` parsing logic, not from re-deriving shapes off the mockup's HTML |
