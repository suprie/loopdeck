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

Placeholder — no decisions made yet in the planning conversation. Fill in as
the spike phase and each PRD's Design section resolve open questions (drawer
routing model, Epics/Graph relocation, whether "night" becomes a `RunState`
variant or a derived UI flag).

### ADR-1: <title> — fill in

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
