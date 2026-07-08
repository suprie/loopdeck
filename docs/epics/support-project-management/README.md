---
title: Support Project Management
slug: support-project-management
milestone: "0.2.0"
status: in_progress
started: 2026-07-08
owner: Suprie
description: >
  Introduce an Epic → PRD → Phase → Loop planning hierarchy so a human can
  structure a large feature as a reviewable, git-tracked spec in docs/epics/,
  then promote its atomic work into .loopdeck/loops.md where the existing
  agent runtime executes it. The agent stays unaware of the hierarchy; the app
  owns the spec layer, the human owns the commit.
---

# Epic — Support Project Management

## Motivation

LoopDeck today executes work as a flat list of loops in `.loopdeck/loops.md`.
That works for single-session iterations but breaks down past ~10 items: the
planning and the execution live in the same file, the plan is treated as
mutable app state, and there is no way to group loops by the outcome they
serve. The audit P2–P6 roadmap in `loops.md` is already exhibiting the failure
mode — a long flat checklist with no grouping, no ownership boundary, no
reviewable spec.

This epic introduces the separation explicitly:

- **`docs/` = intention.** Human-authored, git-tracked, reviewable in PRs,
  readable by both humans and AI via `@`-mention. The plan of record.
- **`.loopdeck/` = execution.** App- and agent-written runtime state — the
  current loop, history, decisions. The truth of what happened.

The bridge between them is a single UI action: **promote a PRD checklist item
into `.loopdeck/loops.md ## Current`**, carrying a back-reference so the
executed loop remembers which epic/PRD/phase it came from.

## Scope

In scope:

- `docs/epics/<slug>/` directory convention — epic README + co-located PRDs,
  each with YAML frontmatter for indexing.
- `epic.rs` parser that reads the spec layer (frontmatter + body, sibling to
  `memory.rs`).
- Cross-project `/epics` view + Epics tab in `ProjectDetail`, grouped by
  milestone via frontmatter.
- The promote-to-loop bridge with the back-reference tag.
- Skill split: `loopdeck-orchestrator` → `loopdeck-loop-runner` +
  `loopdeck-epic-author` + `loopdeck-memory`. Runner/author are mechanics;
  neither owns the plan.
- Managed-skills model: version-aware refresh so the split reaches existing
  projects instead of leaving them on the fat self-directing orchestrator.
- Bootstrap `docs/epics/` for new projects.

Out of scope (deferred to later milestones):

- AI-generated PRDs and AI phase decomposition → **0.3.0** (the
  `loopdeck-epic-author` skill lands as a human-invoked drafting aid in 0.2.0
  and gains the app-invoked PRD-dialogue front-end in 0.3.0).
- Agent awareness of epics (`build_next_loop_prompt` unchanged) → **0.4.0**.
- Git branch-per-epic → **0.4.0**.
- Worktrees / same-repo parallel epics → **0.5.0**.
- Runtime skill injection (the Parking Lot "Move agent control into LoopDeck
  app" item) — supersedes the managed-skills model in a later milestone.

## Non-Goals

- **Auto-sync between the PRD checklist and `loops.md` History.** In 0.2.0 the
  human checks off PRD boxes by hand. Drift between the plan-of-record and the
  truth-of-execution is expected and visible — the human reconciles it.
  Auto-sync is a 0.2.x refinement once the manual rule is proven.
- **Cross-project epics** (an epic spanning two repos). Each epic is scoped to
  one project. A shared roadmap across repos is a later concern.
- **Restructuring the agent's autonomy.** The agent keeps executing the current
  loop exactly as today. The only agent-visible change is the runner skill's
  new read-context rule: follow a loop's epic/prd back-reference as context,
  not as a mandate to reorganize.

## PRD Index

| PRD | Covers |
|-----|--------|
| [prd-spec-layer.md](./prd-spec-layer.md) | `docs/epics/` layout, frontmatter spec, `epic.rs` parser, format contract with the authoring skill |
| [prd-epics-view.md](./prd-epics-view.md) | Cross-project `/epics` view (grouped by milestone), Epics tab, promote-to-loop bridge |
| [prd-skill-split.md](./prd-skill-split.md) | Orchestrator → runner + author + memory; managed-skills refresh; migration |

## Architecture Decisions

### ADR-1: Epics live in `docs/`, not `.loopdeck/`

**Context.** The original proposal put epics in `.loopdeck/epics.md`,
mirroring `decisions.md`. That conflates the plan (intention, authored
deliberately) with runtime state (what the app writes during execution).

**Decision.** Epics and PRDs live under `docs/epics/<slug>/`, committed to git.
The app reads them as ordinary files; the agent reads them via the same
`@`-mention / file-read path as any other repo file. `.loopdeck/` stays the
runtime layer — current loop, history, decisions.

**Consequences.** The plan is reviewable in PRs and survives across runs. AI
drafts (0.3.0) land as committed artifacts, not as drift-prone app state. The
cost is two views of the same work (PRD checklist vs. History) that will
drift; 0.2.0 makes that drift visible rather than hiding it.

### ADR-2: One directory per epic, not a flat file

**Context.** A single `epics.md` would mirror `decisions.md` and simplify the
parser. But epics accumulate PRDs and notes; a flat file scales poorly past a
few entries.

**Decision.** `docs/epics/<slug>/README.md` holds the epic; PRDs are
co-located `prd-<topic>.md` files in the same directory. `ls
docs/epics/support-gemini/` shows everything about that epic.

**Consequences.** Parser must walk a directory, not read one file. Slightly
more code; much better scaling. The directory name is the slug and the
back-reference key.

### ADR-3: YAML frontmatter for spec files, `**Field**` bullets for runtime files

**Context.** The first draft used `**Milestone**: 0.2.0` bullets in the epic
body, mirroring `decisions.md` / `loops.md`. But epics need to be *indexed*
(grouped by milestone, filtered by status) — and that's the job SKILL.md
solves with YAML frontmatter: a small structured header for discovery, prose
body for content. Bullets conflate the two layers.

**Decision.** Spec-layer files (`docs/epics/**/*.md`) carry YAML frontmatter
with the index fields (`title`, `slug`, `milestone`, `status`, `started`,
`completed`, `owner`, `description`). The body is pure prose.
Runtime-layer files (`.loopdeck/loops.md`, `decisions.md`) keep the
`**Field**: value` bullet convention — they're agent-written and lenient.

**Consequences.** The `/epics` view groups by `milestone` via a frontmatter
query, no body parsing. The format difference *reinforces* the layer
separation: structured where humans index, lenient where agents write. The
back-reference the app writes into `loops.md` on promote stays as bullets,
because it's writing into a runtime file. `epic.rs` uses `serde_yaml` (already
in the dep tree) for frontmatter and line-scans only the `### Phase` sections.

### ADR-4: The `loopdeck-` prefix is the skills ownership boundary

**Context.** `copy_skills` skips any skill whose `SKILL.md` already exists, to
preserve user customizations. That makes the orchestrator split unreachable on
existing projects — the fat self-directing version persists forever.

**Decision.** Skills with the `loopdeck-` prefix are app-managed: the app may
overwrite them when its version advances. Skills without the prefix are
user-owned and never touched. A `.claude/skills/.loopdeck-manifest.json`
records what the app installed and at what version.

**Consequences.** Users who want to customize a loopdeck skill copy it to a
new name and edit that — the prefix is a convention, not a lock. The manifest
enables a one-time migration that removes the old `loopdeck-orchestrator`.
Runtime skill injection (Parking Lot) will subsume this later without
re-litigating the ownership rule.

## Success Criteria

- A user can create an epic README + one PRD by hand (or with the authoring
  skill) and see it in the `/epics` view, grouped under its milestone.
- The user can promote a PRD checklist item into `.loopdeck/loops.md` via the
  UI; the promoted loop carries its epic/prd back-reference.
- The agent executes the promoted loop with no change to `build_next_loop_prompt`.
- The `loopdeck-orchestrator` skill no longer exists in a freshly-bootstrapped
  project; `loopdeck-loop-runner`, `loopdeck-epic-author`, and `loopdeck-memory`
  are present instead.
- An existing project (pre-split) can run "Refresh skills" and receive the
  three new skills, with the old orchestrator removed and the action logged.
- LoopDeck's own 0.2.0 work is planned and tracked using this very epic.

## Risks

| Risk | Mitigation |
|------|-----------|
| PRD checklist and `loops.md` History drift silently | 0.2.0 surfaces both in the UI; human reconciles. No auto-sync until the right rule is proven. |
| Back-reference format gets unwieldy as epics grow | Keep it to two fields (`**Epic**` / `**PRD**`); phase is inferred from checklist position. Revisit if it proves insufficient. |
| Managed-skills refresh clobbers a user's in-place edit of a `loopdeck-` skill | Document the prefix convention in CONTRIBUTING; the refresh is version-gated, not every-run. The escape hatch is renaming. |
| The skill split changes agent behavior for existing projects mid-stream | Migration is explicit (logged), user-triggered via Refresh, not silent. The user opts in per project. |
| Frontmatter diverges between hand-authored and AI-drafted epics | The `loopdeck-epic-author` skill carries the frontmatter schema as its format contract; `epic.rs` and the skill ship together. Schema validation on parse surfaces mismatches loudly. |
