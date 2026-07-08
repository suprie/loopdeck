---
prd: prd-spec-layer
epic: support-project-management
milestone: "0.2.0"
status: proposed
description: >
  Define the on-disk format for the spec layer (epics + PRDs under
  docs/epics/<slug>/) and build the Rust parser that reads it. Establishes the
  frontmatter schema that enables milestone grouping and the format contract
  between epic.rs and the loopdeck-epic-author skill.
---

# PRD — Spec Layer: `docs/epics/` + `epic.rs` parser

## Overview

Define the on-disk format for the spec layer (epics + PRDs under
`docs/epics/<slug>/`) and build the Rust parser that reads it. This is the
foundation: the Epics view, the promote-to-loop bridge, and the authoring
skill all depend on a stable, parseable format. Get the format right here and
everything downstream is mechanical.

This PRD also establishes the **format contract** between `epic.rs` (what the
app reads) and `loopdeck-epic-author` (what the AI writes). They must agree on
the same frontmatter schema and body structure, or hand-authored and
AI-drafted epics parse inconsistently. They ship in the same binary, so they
don't drift within a release.

## Problem Statement

There is no spec layer today. `.loopdeck/loops.md` is the only planning
artifact, and it mixes plan + execution in one flat file. `memory.rs` parses
`decisions.md` and `loops.md` but has no concept of an epic. The agent's
`loopdeck-orchestrator` skill self-directs planning because there's no
external plan for it to read.

A second problem surfaced during drafting: the first format proposal used
`**Milestone**: 0.2.0` bullets in the epic body, mirroring the runtime-file
convention. But epics need to be *indexed* — grouped by milestone, filtered by
status — and that's structurally the same job SKILL.md solves with YAML
frontmatter. Bullets conflate the index layer with the content layer.

## Goals

| Priority | Goal |
|----------|------|
| P0 | A documented `docs/epics/<slug>/` directory layout — epic README + co-located PRDs, each with YAML frontmatter |
| P0 | A frontmatter schema that enables grouping by milestone and filtering by status without body parsing |
| P0 | `epic.rs` with `parse_epics(project_path) -> Vec<Epic>` and `parse_prd(path) -> Prd` |
| P0 | Epic and PRD structs that carry the frontmatter fields + the phase checklists from the body |
| P1 | `ensure_memory_files`-style bootstrap: new projects get an empty `docs/epics/` |
| P1 | Lenient body parsing — missing `## Scope`, em dashes in prose, empty `docs/epics/` don't panic (mirror `memory.rs`); frontmatter is strict (serde_yaml) |

## Non-Goals

- Writing epics or PRDs from the app — humans and the authoring skill do that.
  The app only reads and displays.
- Promoting loops (that's `prd-epics-view.md`).
- Parsing the existing flat `docs/PRD-*.md` files into the new model — those
  are legacy and stay where they are. Only `docs/epics/` is the new layer.
- Strict schema validation that rejects unknown frontmatter fields — accept
  extras, require only the known ones. Forward-compatibility over strictness.

## Format Spec

### Layer convention (reinforces ADR-1 + ADR-3)

| Layer | Location | Format | Parsed by |
|---|---|---|---|
| Spec | `docs/epics/**/*.md` | YAML frontmatter + prose body | `epic.rs` (serde_yaml frontmatter, line-scan body) |
| Runtime | `.loopdeck/*.md` | `**Field**: value` bullets | `memory.rs` (lenient line-scan) |

The format difference is the layer's job: spec files are indexed (need
structured fields), runtime files are written by agents (need lenience).

### Frontmatter schema (epic README)

```yaml
---
title: Support Project Management      # human-readable
slug: support-project-management       # kebab-case; MUST match directory name
milestone: "0.2.0"                     # quoted to keep it a string, not a float
status: in_progress                    # proposed | in_progress | completed | abandoned
started: 2026-07-08                    # ISO date
completed: 2026-07-20                  # omit unless status is completed
owner: Suprie
description: >                         # one-paragraph goal; folded scalar
  Introduce an Epic → PRD → Phase → Loop planning hierarchy...
---
```

Required: `title`, `slug`, `milestone`, `status`, `description`.
Optional: `started`, `completed`, `owner`. Extras ignored.

`milestone` is quoted because YAML parses `0.2.0` as a float (`0.2`) without
quotes. This is the kind of detail the format contract exists to nail down.

### Frontmatter schema (PRD)

```yaml
---
prd: prd-spec-layer                    # filename without .md
epic: support-project-management       # parent epic slug
milestone: "0.2.0"
status: proposed                       # proposed | accepted | completed
description: >
  Define the on-disk format for the spec layer...
---
```

Required: `prd`, `epic`, `status`, `description`. `milestone` is inherited
from the epic but denormalized here so the `/epics` view can group/filter PRDs
without joining back to the epic README.

### Body structure

Epic README body: `## Motivation`, `## Scope`, `## Non-Goals`, `## PRD Index`
(table), `## Architecture Decisions`, `## Success Criteria`, `## Risks`.
Headings are conventional, not enforced — `epic.rs` reads only the PRD Index
table from the body.

PRD body: `## Overview`, `## Problem Statement`, `## Goals`, `## Non-Goals`,
`## Phases`. Only `## Phases` is structurally parsed:

```markdown
## Phases

### Phase 1 — <Name>
- [ ] <loop title>
- [ ] <loop title>

### Phase 2 — <Name>
- [ ] <loop title>
```

A phase is a `### Phase N — Name` heading followed by a GFM checklist. Each
unchecked `- [ ]` item is a **planned loop** — the atomic unit the
promote-to-loop action acts on. Checked `- [x]` items are done.

### Back-reference (written into `loops.md` on promote — runtime layer, bullets)

When a PRD checklist item is promoted into `.loopdeck/loops.md ## Current`, the
promoted entry carries bullets (runtime convention, not frontmatter):

```markdown
## Current

- **Started**: <today>
- **Goal**: <the checklist item text>
- **Status**: in_progress
- **Epic**: <slug>
- **PRD**: <prd-filename-without-.md>
```

This reuses the existing `## Current` shape that `build_next_loop_prompt` and
`read_current_loop` already read — they ignore the unknown `**Epic**`/`**PRD**`
fields. The agent's prompt is built from `**Goal**` exactly as today. The
runner skill's read-context rule follows the bullet back-references to load
the origin PRD as context.

## Phases

### Phase 1 — Core structs and parser

- [ ] Define `Epic`, `Prd`, `PrdPhase`, `PrdLoop` structs in `epic.rs` with serde derives; frontmatter fields via `serde_yaml`
- [ ] Implement frontmatter extractor: split `---\n...\n---` from body, deserialize with `serde_yaml`
- [ ] Implement `parse_epic_readme(path) -> Epic` — frontmatter + PRD Index table from body
- [ ] Implement `parse_prd(path) -> Prd` — frontmatter + `### Phase N` sections into `Vec<PrdPhase>` with checklist items
- [ ] Implement `parse_epics(project_path) -> Vec<Epic>` — walk `docs/epics/*/`, parse each README, attach parsed PRDs
- [ ] Milestone grouping: `epics_by_milestone(project_path) -> BTreeMap<String, Vec<Epic>>` (ordered by milestone)
- [ ] Lenient body edge cases: missing `## Scope`, missing PRD Index table, em dashes in prose, empty `docs/epics/`
- [ ] Strict frontmatter: missing required field → parse error with the file path (not a panic)
- [ ] Unit tests mirroring `memory.rs` coverage (≥15 tests) + frontmatter round-trip tests

### Phase 2 — Bootstrap and integration

- [ ] Add `docs/epics/` to the bootstrap path in `project.rs` (create-on-absent, idempotent)
- [ ] Register `get_epics(project_path)` + `get_epics_by_milestone(project_path)` Tauri commands in `commands.rs` + `lib.rs`
- [ ] Typed `getEpics` / `getEpicsByMilestone` wrappers in `src/lib/tauri.ts` + `Epic`/`Prd`/`PrdPhase`/`PrdLoop` types in `src/types/index.ts`
- [ ] Verify LoopDeck's own `docs/epics/support-project-management/` parses cleanly (dogfood)

## Open Questions

- Should `parse_epics` return PRDs nested under each epic, or as a separate
  `get_prds(epic_slug)` call? **Lean:** nested — the UI renders epic → PRD →
  phase as a tree, one fetch. Revisit if a PRD gets large enough that eager
  parsing is slow.
- Should `milestone` on the PRD be validated against the epic's milestone?
  **Lean:** no in 0.2.0 — denormalization is for query convenience, and a
  mismatch would surface in the UI (PRD under the wrong milestone group).
  Add a lint later if it bites.
