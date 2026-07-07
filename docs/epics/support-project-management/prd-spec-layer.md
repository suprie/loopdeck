# PRD — Spec Layer: `docs/epics/` + `epic.rs` parser

**Epic**: support-project-management
**Status**: Proposed (2026-07-08)

## Overview

Define the on-disk format for the spec layer (epics + PRDs under
`docs/epics/<slug>/`) and build the Rust parser that reads it. This is the
foundation: the Epics view, the promote-to-loop bridge, and the authoring
skill all depend on a stable, parseable format. Get the format right here and
everything downstream is mechanical.

This PRD also establishes the **format contract** between `epic.rs` (what the
app reads) and `loopdeck-epic-author` (what the AI writes). They must agree on
the same headings and field syntax, or hand-authored and AI-drafted epics
parse inconsistently. They ship in the same binary, so they don't drift within
a release.

## Problem Statement

There is no spec layer today. `.loopdeck/loops.md` is the only planning
artifact, and it mixes plan + execution in one flat file. `memory.rs` parses
`decisions.md` and `loops.md` but has no concept of an epic. The agent's
`loopdeck-orchestrator` skill self-directs planning because there's no
external plan for it to read.

## Goals

| Priority | Goal |
|----------|------|
| P0 | A documented `docs/epics/<slug>/` directory layout — epic README + co-located PRDs |
| P0 | `epic.rs` with `parse_epics(project_path) -> Vec<Epic>` and `parse_prd(path) -> Prd` |
| P0 | Epic and PRD structs that carry the fields the UI needs (status, goal, phase checklists) |
| P1 | `ensure_memory_files`-style bootstrap: new projects get an empty `docs/epics/` |
| P1 | Lenient parsing — missing fields, em dashes, missing headings don't panic (mirror `memory.rs`) |

## Non-Goals

- Writing epics or PRDs from the app — humans and the authoring skill do that.
  The app only reads and displays.
- Promoting loops (that's `prd-epics-view.md`).
- Parsing the existing flat `docs/PRD-*.md` files into the new model — those
  are legacy and stay where they are. Only `docs/epics/` is the new layer.

## Format Spec

### Epic README — `docs/epics/<slug>/README.md`

```markdown
# Epic — <Title>

- **Milestone**: 0.2.0
- **Status**: in_progress | proposed | completed | abandoned
- **Started**: YYYY-MM-DD
- **Completed**: YYYY-MM-DD        # omitted unless completed
- **Goal**: <one paragraph>
- **Owner**: <name>

## Scope
...

## Non-Goals
...

## PRD Index

| PRD | Covers |
|-----|--------|
| [prd-<topic>.md](./prd-<topic>.md) | <summary> |
```

The directory name (`<slug>`) is the epic's identity and the back-reference
key. Slug rules: lowercase, kebab-case, no spaces.

### PRD — `docs/epics/<slug>/prd-<topic>.md`

```markdown
# PRD — <Title>

**Epic**: <slug>
**Status**: Proposed | Accepted | Completed

## Overview
...

## Phases

### Phase 1 — <Name>
- [ ] <loop title>
- [ ] <loop title>

### Phase 2 — <Name>
- [ ] <loop title>
```

A phase is a `### Phase N — Name` heading followed by a GFM checklist. Each
unchecked `- [ ]` item is a **planned loop** — the atomic unit the
promote-to-loop action acts on.

### Back-reference (written into `loops.md` on promote)

When a PRD checklist item is promoted into `.loopdeck/loops.md ## Current`, the
promoted entry carries:

```markdown
- **Title**: <the checklist item text>
- **Epic**: <slug>
- **PRD**: <prd-filename-without-.md>
```

The runner skill's read-context rule follows `**Epic**`/`**PRD**` to load the
origin PRD as context before executing. No other fields — phase is inferred
from the checklist position the item was promoted from.

## Phases

### Phase 1 — Core structs and parser

- [ ] Define `Epic`, `EpicLoop`, `Prd`, `PrdPhase` structs in `epic.rs` with serde derives
- [ ] Implement `parse_epic_readme(path) -> Epic` — read the `**Field**: value` header block + PRD index table
- [ ] Implement `parse_prd(path) -> Prd` — read overview + `### Phase N` sections into `Vec<PrdPhase>` with checklist items
- [ ] Implement `parse_epics(project_path) -> Vec<Epic>` — walk `docs/epics/*/`, parse each README, attach parsed PRDs
- [ ] Lenient edge cases: missing `## Scope`, missing PRD index table, em dashes in fields, empty `docs/epics/`
- [ ] Unit tests mirroring `memory.rs` coverage (≥15 tests)

### Phase 2 — Bootstrap and integration

- [ ] Add `docs/epics/` to the bootstrap path in `project.rs` (create-on-absent, idempotent)
- [ ] Register `get_epics(project_path)` Tauri command in `commands.rs` + `lib.rs`
- [ ] Typed `getEpics` wrapper in `src/lib/tauri.ts` + `Epic`/`Prd`/`PrdPhase` types in `src/types/index.ts`
- [ ] Verify LoopDeck's own `docs/epics/support-project-management/` parses cleanly (dogfood)

## Open Questions

- Should `parse_epics` return PRDs nested under each epic, or as a separate
  `get_prds(epic_slug)` call? **Lean:** nested — the UI renders epic → PRD →
  phase as a tree, one fetch. Revisit if a PRD gets large enough that eager
  parsing is slow.
