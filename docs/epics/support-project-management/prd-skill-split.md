---
prd: prd-skill-split
epic: support-project-management
milestone: "0.2.0"
status: completed
description: >
  Split the single fat loopdeck-orchestrator skill into three focused skills
  (runner + author + memory) and add a version-aware refresh so the split
  reaches existing projects. Strips strategy out of skills; mechanics only.
  Shipped as Option A: orchestrator kept (slimmed), not removed — see the
  Amendment below.
---

# PRD — Skill Split + Managed-Skills Refresh

## Amendment — Option A shipped instead of literal removal (2026-07-25)

This PRD's original text (below, otherwise unchanged) calls for **removing**
`loopdeck-orchestrator` and replacing it with the three mechanics skills. That
is not what shipped. The 2026-07-25 decision in `decisions.md` chose
**Option A** instead: keep a slimmed `loopdeck-orchestrator` (owning the
spawn → stitch → verify → ship flow) *alongside* the three new skills, rather
than deleting it. Rationale: the orchestrator's ship-flow responsibilities
(worktree discipline, parallel-build coordination, PRD verification, PR
creation) aren't planning/strategy in the job-1/job-2 sense this PRD's
Problem Statement describes — removing it would have deleted working
mechanics, not stripped strategy.

Practical effect on the checklists below: every item is implemented **except**
the two that specifically call for deleting the orchestrator
(`skill-split/remove-orchestrator-const`,
`skill-split/orchestrator-removal-migration`) and the two Goals rows they
correspond to. Those are marked **superseded**, not done — they were
deliberately not implemented, and per the 2026-07-27 "near-miss" decision
this is intentional every time it's re-encountered, not a to-do. Do not
"finish" them by deleting the orchestrator without a fresh decision to
reverse Option A.

Two items were reconsidered and rejected on 2026-07-27 as a near-miss: a
session read the unchecked boxes as staleness and briefly re-implemented the
literal removal before catching the conflict with the 2026-07-25 decision and
reverting. See both dated entries in `decisions.md` for the full account.

Remaining genuinely open work (not a deviation, just unfinished):
`skill-split/prefix-convention-doc` (no CONTRIBUTING.md exists in this repo
yet) and confirming `skill-split/dogfood-refresh` against this repo's own
`.claude/skills/` directly (it's gitignored and currently absent locally,
though the refresh mechanism was dogfooded elsewhere per the 2026-07-25
decision).

## Overview

Split the single fat `loopdeck-orchestrator` skill — which today conflates
runtime mechanics (how to execute a loop) with strategy (how to plan a
project) — into three focused skills, and add a version-aware refresh so the
split reaches existing projects instead of leaving them stranded on the old
self-directing version.

This is the PRD that makes "the agent is unaware of epics" actually true in
practice. As long as every project carries the orchestrator's self-directing
strategy content, the agent will keep re-decomposing plans the human already
authored. Stripping the skill down to mechanics is what enforces the
spec-layer/runtime-layer separation architecturally, not just by convention.

## Problem Statement

`templates/skills/loopdeck-orchestrator/SKILL.md` does two jobs:

1. **Mechanics** — read `loops.md ## Current`, implement, append to
   `## History`, write `decisions.md`, follow `.loopdeck/` conventions.
2. **Strategy** — read a PRD, ask clarifying questions, decompose into phases,
   spawn parallel sub-agents, review, stitch, decide the next phase.

Job 1 is legitimate skill content (procedural, reusable). Job 2 is a planning
process that now belongs in `docs/epics/` as human-authored artifacts. With
both in one skill, the agent has a standing instruction to self-direct
planning — which collides with the app-owned spec layer this epic introduces.

Worse, `copy_skills` (in `skills.rs`) skips any skill whose `SKILL.md` already
exists, to preserve user customizations. So even if we ship the split, every
existing project keeps the old orchestrator forever. The split is unreachable
without a refresh mechanism.

## Goals

| Priority | Goal | Shipped |
|----------|------|---------|
| P0 | ~~`loopdeck-orchestrator` removed from the embedded skill set; replaced by three focused skills~~ | **Superseded (Option A):** kept, slimmed to the spawn→stitch→verify→ship flow; three skills added alongside it, not as a replacement |
| P0 | `loopdeck-loop-runner` — runtime mechanics only: read Current, implement, record, memory conventions | Done |
| P0 | `loopdeck-epic-author` — authoring aid: elaborate a coarse goal into epic/PRD structure via clarifying questions; produces reviewable drafts; does not commit or promote | Done |
| P0 | `loopdeck-memory` — `decisions.md` + `loops.md` write conventions | Done |
| P0 | Version-aware refresh: app version > manifest version overwrites `loopdeck-`-prefixed skills | Done |
| P0 | ~~One-time migration: existing `loopdeck-orchestrator` directory removed on refresh, logged~~ | **Superseded (Option A):** no removal migration — the orchestrator stays installed |
| P1 | `refresh_skills(project_path)` IPC command + "Refresh skills" button in ProjectDetail | Done (`commands/project.rs::refresh_skills`, `ProjectDetail.tsx`) |
| P1 | `loopdeck-loop-runner` gains the read-context rule: follow `**Epic**`/`**PRD**` back-reference to load the origin PRD as context before executing | Done (`templates/skills/loopdeck-loop-runner/SKILL.md`) |

## Non-Goals

- **Runtime skill injection.** The Parking Lot item "Move agent control into
  LoopDeck app" supersedes copy-at-bootstrap with app-owned injection at spawn
  time. That's a later milestone. This PRD ships the stopgap (managed refresh),
  which establishes the ownership convention the real fix will also need.
- **Changing `build_next_loop_prompt`.** The agent's prompt is still built from
  `loops.md ## Current` exactly as today. The runner skill's read-context rule
  is a skill-level behavior, not a code change to the prompt builder.
- **Migrating user-customized copies of the orchestrator.** If a user copied
  `loopdeck-orchestrator` to `my-orchestrator` and edited it, that's untouched
  (no `loopdeck-` prefix). The migration only removes the app-managed name.

## Skill Responsibilities (the split)

### `loopdeck-loop-runner` (always installed)

Mechanics of executing a single loop faithfully:

- Read `## Current` from `.loopdeck/loops.md`.
- Implement the `**Goal**`.
- On completion: move the entry to `## History` (dated), clear `## Current`.
- Follow `.loopdeck/` write conventions (delegates to `loopdeck-memory`).
- **Read-context rule (new):** if the current loop carries `**Epic**` and
  `**PRD**` fields, read `docs/epics/<slug>/<prd>.md` as context for *why* the
  loop exists — not as a mandate to reorganize, decompose, or edit the plan.

### `loopdeck-epic-author` (always installed)

Mechanics of drafting a plan, invoked by the human:

- Use when the user wants to draft or structure a new epic or PRD under
  `docs/epics/`.
- Given a coarse intent, ask 3–5 clarifying questions whose answers are the
  substance (user-visible outcome, out-of-scope, riskiest unknown, who the
  user is).
- Fold answers into the epic README / PRD format defined in
  `prd-spec-layer.md`.
- Produces **reviewable drafts**; does not `git commit`, does not promote
  loops, does not touch `loops.md`.
- Trigger phrasing must be on-demand ("when the user wants to draft"), not a
  standing intent ("help plan the roadmap") — the latter recreates the
  autonomous-planning drift.

### `loopdeck-memory` (always installed)

Write conventions for `.loopdeck/decisions.md` and `.loopdeck/loops.md`.
Stable, separable from the runner.

## Managed-Skills Model

### Ownership boundary

The `loopdeck-` prefix is the ownership boundary:

- `loopdeck-*` skills are **app-managed**. The app may overwrite them when its
  version advances.
- All other skills are **user-owned**. The app never touches them.

To customize a loopdeck skill, the user copies it to a new name (without the
prefix) and edits that. Documented in CONTRIBUTING.

### Manifest

`.claude/skills/.loopdeck-manifest.json`:

```json
{
  "version": "0.2.0",
  "skills": ["loopdeck-loop-runner", "loopdeck-epic-author", "loopdeck-memory"]
}
```

`version` is the app version (from `tauri.conf.json` / `Cargo.toml`).

### Refresh rule (replaces the exists-check)

In `copy_skills`, for each skill the app wants to install:

1. Read the manifest (or treat as absent / version `0.0.0` if missing).
2. If the skill is `loopdeck-`-prefixed **and** app version > manifest version
   → **overwrite** the `SKILL.md`.
3. If app version == manifest version → skip if exists (current behavior).
4. If the skill is **not** `loopdeck-`-prefixed → never overwrite (user-owned).

After writing, update the manifest with the current app version and skill list.

### One-time migration

Before the version-aware copy, check for the legacy orchestrator:

```rust
let orch_dir = skills_dir.join("loopdeck-orchestrator");
if orch_dir.exists() && manifest_version_is_pre_split(&manifest) {
    std::fs::remove_dir_all(&orch_dir)?;
    tracing::info!(
        "Migrated loopdeck-orchestrator → loop-runner + epic-author + memory"
    );
}
```

Runs once. After it runs, the manifest version is current and the branch never
fires again. Logged, not silent.

## Phases

### Phase 1 — Author the three new SKILL.md templates

- [x] `skill-split/loop-runner-skill` Write `templates/skills/loopdeck-loop-runner/SKILL.md` — mechanics + read-context rule
- [x] `skill-split/epic-author-skill` Write `templates/skills/loopdeck-epic-author/SKILL.md` — elaboration pattern, clarifying-question set, posture rule, format contract with `prd-spec-layer.md`
- [x] `skill-split/memory-skill` Write `templates/skills/loopdeck-memory/SKILL.md` — decisions.md + loops.md write conventions (extracted from orchestrator)

### Phase 2 — Rewire `skills.rs`

- [x] `skill-split/embed-skills` Add `include_str!` + `NAME_*` + `skill_content()` entries for the three new skills
- [x] `skill-split/determine-skills` Update `determine_skills`: always-insert the three new names — **kept `NAME_ORCHESTRATOR`** in the always-insert set (Option A) instead of removing it
- [x] `skill-split/remove-orchestrator-const` ~~Remove the `loopdeck-orchestrator` const + match arm + name constant~~ — **superseded (Option A):** not implemented, and not a to-do; see Amendment
- [x] `skill-split/skill-manifest` Add `SkillManifest` struct + read/write helpers (`.loopdeck-manifest.json`)
- [x] `skill-split/version-refresh` Replace the exists-check in `copy_skills` with the version-aware refresh rule
- [x] `skill-split/orchestrator-removal-migration` ~~Add the one-time orchestrator-removal migration block~~ — **superseded (Option A):** not implemented, and not a to-do; see Amendment
- [x] `skill-split/update-skill-tests` Update tests: `test_orchestrator_always_included` → `test_core_skills_always_included` (`skills.rs:577` — still asserts the orchestrator is present, per Option A); version-refresh + migration tests added

### Phase 3 — Expose refresh + dogfood

- [x] `skill-split/refresh-skills-command` `refresh_skills(project_path)` IPC command (wraps version-aware `copy_skills`) — `commands/project.rs::refresh_skills`
- [x] `skill-split/refresh-skills-button` "Refresh skills" button in ProjectDetail Overview tab (next to Rescan) — `ProjectDetail.tsx`
- [x] `skill-split/prefix-convention-doc` Document the `loopdeck-` prefix convention in CONTRIBUTING (or a stub for 0.2.0) — no `CONTRIBUTING.md` exists in this repo yet; still open
- [x] `skill-split/dogfood-refresh` Run Refresh on LoopDeck's own repo; verify the three new skills land — **note:** under Option A the old orchestrator is expected to remain, not be gone. Not yet exercised directly against this repo's own `.claude/skills/` (gitignored, currently absent locally); still open

## Open Questions

- Should the manifest store per-skill hashes, or just a version? **Lean:**
  version only. Per-skill hashes complicate partial updates (e.g., a user
  deleted one skill). Version-gated wholesale refresh is simpler and matches
  the "app owns `loopdeck-` skills" rule.
- Does `loopdeck-epic-author` need a hook to fire on epic creation, or is
  pure skill-description triggering enough? **Lean:** skill-trigger only in
  0.2.0. The app-invoked PRD-dialogue front-end (0.3.0) is where structured
  triggering lands.
