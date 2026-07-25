---
prd: prd-skill-split
epic: support-project-management
milestone: "0.2.0"
status: proposed
description: >
  Split the single fat loopdeck-orchestrator skill into three focused skills
  (runner + author + memory) and add a version-aware refresh so the split
  reaches existing projects. Strips strategy out of skills; mechanics only.
---

# PRD — Skill Split + Managed-Skills Refresh

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

| Priority | Goal |
|----------|------|
| P0 | `loopdeck-orchestrator` removed from the embedded skill set; replaced by three focused skills |
| P0 | `loopdeck-loop-runner` — runtime mechanics only: read Current, implement, record, memory conventions |
| P0 | `loopdeck-epic-author` — authoring aid: elaborate a coarse goal into epic/PRD structure via clarifying questions; produces reviewable drafts; does not commit or promote |
| P0 | `loopdeck-memory` — `decisions.md` + `loops.md` write conventions |
| P0 | Version-aware refresh: app version > manifest version overwrites `loopdeck-`-prefixed skills |
| P0 | One-time migration: existing `loopdeck-orchestrator` directory removed on refresh, logged |
| P1 | `refresh_skills(project_path)` IPC command + "Refresh skills" button in ProjectDetail |
| P1 | `loopdeck-loop-runner` gains the read-context rule: follow `**Epic**`/`**PRD**` back-reference to load the origin PRD as context before executing |

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

- [ ] `skill-split/loop-runner-skill` Write `templates/skills/loopdeck-loop-runner/SKILL.md` — mechanics + read-context rule
- [ ] `skill-split/epic-author-skill` Write `templates/skills/loopdeck-epic-author/SKILL.md` — elaboration pattern, clarifying-question set, posture rule, format contract with `prd-spec-layer.md`
- [ ] `skill-split/memory-skill` Write `templates/skills/loopdeck-memory/SKILL.md` — decisions.md + loops.md write conventions (extracted from orchestrator)

### Phase 2 — Rewire `skills.rs`

- [ ] `skill-split/embed-skills` Add `include_str!` + `NAME_*` + `skill_content()` entries for the three new skills
- [ ] `skill-split/determine-skills` Update `determine_skills`: always-insert the three new names; remove `NAME_ORCHESTRATOR`
- [ ] `skill-split/remove-orchestrator-const` Remove the `loopdeck-orchestrator` const + match arm + name constant
- [ ] `skill-split/skill-manifest` Add `SkillManifest` struct + read/write helpers (`.loopdeck-manifest.json`)
- [ ] `skill-split/version-refresh` Replace the exists-check in `copy_skills` with the version-aware refresh rule
- [ ] `skill-split/orchestrator-removal-migration` Add the one-time orchestrator-removal migration block
- [ ] `skill-split/update-skill-tests` Update tests: `test_orchestrator_always_included` → `test_core_skills_always_included`; add version-refresh + migration tests

### Phase 3 — Expose refresh + dogfood

- [ ] `skill-split/refresh-skills-command` `refresh_skills(project_path)` IPC command (wraps version-aware `copy_skills`)
- [ ] `skill-split/refresh-skills-button` "Refresh skills" button in ProjectDetail Overview tab (next to Rescan)
- [ ] `skill-split/prefix-convention-doc` Document the `loopdeck-` prefix convention in CONTRIBUTING (or a stub for 0.2.0)
- [ ] `skill-split/dogfood-refresh` Run Refresh on LoopDeck's own repo; verify the three new skills land and the old orchestrator is gone

## Open Questions

- Should the manifest store per-skill hashes, or just a version? **Lean:**
  version only. Per-skill hashes complicate partial updates (e.g., a user
  deleted one skill). Version-gated wholesale refresh is simpler and matches
  the "app owns `loopdeck-` skills" rule.
- Does `loopdeck-epic-author` need a hook to fire on epic creation, or is
  pure skill-description triggering enough? **Lean:** skill-trigger only in
  0.2.0. The app-invoked PRD-dialogue front-end (0.3.0) is where structured
  triggering lands.
