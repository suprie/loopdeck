---
prd: prd-structured-execution-state
epic: support-project-management
milestone: "0.2.1"
status: accepted
accepted: "2026-07-25"
description: >
  Move mutable loop execution state out of Markdown into a schema-versioned
  .loopdeck/execution.yaml file, add stable loop IDs to the spec-to-execution
  bridge, and derive PRD, epic, and shipping progress without duplicating state.
---

# PRD — Structured Execution State + Derived Progress

## Overview

Keep Markdown as LoopDeck's human-readable specification and decision format,
but stop using Markdown as the database for mutable loop execution state.

This refinement introduces:

1. Stable loop IDs in PRD checklist items.
2. A schema-versioned `.loopdeck/execution.yaml` as the single source of truth
   for queued, current, and completed loops.
3. Derived PRD and epic progress in the UI.
4. A backward-compatible migration from `.loopdeck/loops.md`.
5. Optional Git shipping evidence that remains separate from implementation
   completion.

The product boundary becomes:

| Concern | Authoritative source |
|---|---|
| Human intent, scope, and acceptance criteria | `docs/epics/**/*.md` |
| Current, queued, and completed execution | `.loopdeck/execution.yaml` |
| Architectural rationale | `.loopdeck/decisions.md` |
| Conversation transcript | `.loopdeck/sessions/*.jsonl` |
| Commit, review, and release evidence | Git, optional remote-provider metadata |
| Status shown in the UI | Derived by LoopDeck from the sources above |

Markdown remains a first-class product feature. It is no longer responsible
for fields that require stable identity, atomic transitions, or cross-file
joins.

## Problem Statement

LoopDeck currently represents the same work in several mutable forms:

- A PRD checklist item represents planned work.
- `.loopdeck/loops.md ## Current` represents active work.
- `.loopdeck/loops.md ## History` represents completed work.
- The PRD checkbox may be manually checked.
- Epic and PRD frontmatter carry manually maintained status.
- Git independently records whether the implementation was committed or
  shipped.

The 0.2.0 implementation enriches PRD loops with `done_in_history` by matching
the epic, PRD, and loop title. Title editing therefore breaks identity. Manual
checkbox updates and frontmatter status create additional copies of completion
state that can disagree with History.

The Markdown parser is intentionally lenient, which is useful for prose but
weak for a state machine. Section names, bullet syntax, heading placement, and
title text currently participate in application behavior. Adding more derived
progress on top of those conventions would deepen the ambiguity.

LoopDeck needs a structured execution contract before adding automatic
progress, more automation, or additional agent backends.

## Product Principles

1. **One authority per fact.** Intention, execution, and shipping are different
   facts and must not be copied into multiple writable fields.
2. **Markdown stays human-owned.** Specs and decisions remain ordinary,
   reviewable Markdown.
3. **Execution transitions are atomic.** Completing a loop must remove
   `current` and append its history entry in one file replacement.
4. **Identity does not depend on prose.** Titles may change without breaking
   the relationship between a PRD item and its execution record.
5. **Local-first remains non-negotiable.** State stays inspectable,
   git-diffable, repairable, and usable without a database or network.
6. **Derived status is not written back by default.** The UI calculates
   progress; it does not continuously rewrite human-authored PRDs.
7. **Migration is reversible during 0.2.x.** Existing projects continue to
   open, and their original `loops.md` is retained until migration is accepted.

## Goals

| Priority | Goal |
|---|---|
| P0 | Add a stable, project-scoped ID to every promotable PRD loop |
| P0 | Make `.loopdeck/execution.yaml` the canonical queued/current/history store |
| P0 | Promote and complete loops through validated, atomic state transitions |
| P0 | Derive planned, queued, in-progress, completed, implemented, and shipped states without title matching |
| P0 | Migrate existing `loops.md` data without losing current, next-step, or history content |
| P1 | Show migration conflicts and unmatched legacy records instead of guessing |
| P1 | Update bundled skills/hooks so agents follow the structured execution contract |
| P1 | Preserve read compatibility with existing `loops.md` throughout 0.2.x |
| P2 | Enrich completed loops with local Git commit evidence |
| P2 | Enrich shipping state from a remote provider when available, without making it a runtime dependency |

## Non-Goals

- Moving epics, PRDs, decisions, or project descriptions out of Markdown.
- Introducing SQLite or another embedded database.
- Building GitHub-, GitLab-, or Bitbucket-specific authentication in this PRD.
- Automatically editing PRD checkbox syntax when execution completes.
- Inferring identity with fuzzy title matching.
- Reconstructing perfect provenance for ambiguous legacy History entries.
- Cross-project loops or epics.
- Scheduling or unattended loop automation.
- Replacing conversation JSONL storage.

## Stable Loop Identity

### Authoring format

Every promotable PRD checklist item carries a stable ID before its title:

```markdown
### Phase 2 — Promotion

- [ ] `epics-view/promote-loop` Add the Promote button
- [ ] `epics-view/clobber-guard` Prevent replacing an active loop
```

The ID is:

- Unique within a project.
- Lowercase kebab-case with one `/` separating a PRD-local namespace from the
  loop slug.
- Immutable after the loop has been promoted.
- Authored by the human or `loopdeck-epic-author`.
- Stored independently from the display title by `epic.rs`.

Recommended construction:

```text
<prd-short-slug>/<loop-short-slug>
```

The parser rejects duplicate IDs within the same project and surfaces the file
and line of both definitions. A legacy checklist item without an ID remains
readable but cannot be promoted until the user accepts or edits a suggested ID.

### Runtime reference

The same ID is copied into the execution record on promotion. Epic, PRD, and
phase references are retained for navigation and human inspection, but the ID
is the join key.

Titles are presentation. IDs are identity.

## Execution File Contract

### Location

```text
.loopdeck/execution.yaml
```

### Schema

```yaml
schema_version: 1
revision: 12

current:
  id: epics-view/promote-loop
  title: Add the Promote button
  origin:
    epic: support-project-management
    prd: prd-epics-view
    phase: promotion
  status: in_progress
  started_at: 2026-07-25T10:30:00+07:00

queue:
  - id: epics-view/clobber-guard
    title: Prevent replacing an active loop
    origin:
      epic: support-project-management
      prd: prd-epics-view
      phase: promotion
    queued_at: 2026-07-25T10:20:00+07:00

history:
  - id: epics-view/render-milestones
    title: Render milestone groups
    origin:
      epic: support-project-management
      prd: prd-epics-view
      phase: cross-project-view
    outcome: completed
    started_at: 2026-07-24T09:00:00+07:00
    completed_at: 2026-07-24T12:15:00+07:00
    git:
      commit: e368e98
```

### Why one YAML file

Current state and completion history deliberately live in one file for the
first structured-state version. A completion operation must atomically:

1. Validate the active loop ID.
2. Append the completed record to `history`.
3. Clear `current`.
4. Optionally promote the first queued item.
5. Increment `revision`.
6. Atomically replace the file.

Splitting current state into YAML and history into JSONL would require a
cross-file transaction or recovery protocol. That complexity is not justified
at current history volumes. If history size becomes material, a later schema
version may introduce an append-only archive with an explicit checkpoint.

### Validation

- Unknown fields are preserved when possible for forward compatibility.
- Unknown `schema_version` values fail read-only with a clear upgrade message;
  LoopDeck must never rewrite a newer schema.
- Duplicate IDs across `current`, `queue`, and successful `history` records are
  rejected, except for explicit retries with separate attempt numbers.
- `revision` is checked before write to prevent stale UI actions from silently
  overwriting a newer state.
- Writes use the existing atomic-write and backup path.
- A malformed primary file opens the last-known-good backup read-only and asks
  the user before restoration.

## State Model

### Execution state

| Condition | Derived state |
|---|---|
| ID exists only in a PRD | `planned` |
| ID exists in `queue` | `queued` |
| ID equals `current.id` | `in_progress` |
| ID has a successful History record | `completed` |
| ID has an abandoned History record and no later attempt | `abandoned` |
| Legacy record cannot be mapped to an ID | `unmatched` |

### Delivery state

Delivery is related to execution but does not replace it:

| Evidence | Derived delivery state |
|---|---|
| Completed with no commit evidence | `implemented` |
| Referenced local commit exists | `committed` |
| Optional provider reports an open PR containing the commit | `in_review` |
| Optional provider reports the PR merged, or a release tag contains the commit | `shipped` |

Remote-provider data is enrichment. Offline use continues to show the strongest
local state it can prove.

### PRD and epic progress

PRD progress is calculated from its required loop IDs:

```text
completed required loops / total required loops
```

Epic progress aggregates its PRDs. Frontmatter `status` remains an authorial
lifecycle field during migration, but it is no longer interpreted as proof of
execution completion.

Longer term, frontmatter should use:

```yaml
lifecycle: draft | active | cancelled
```

LoopDeck derives `not_started`, `in_progress`, `implemented`, and `shipped`.
This PRD may read legacy `status`, but migration must not silently rewrite spec
frontmatter.

## Write Ownership

`execution.yaml` is app-managed structured state, but agent workflows still
need to complete loops. The supported write paths are:

1. Tauri commands invoked by the UI.
2. A small LoopDeck-owned state command used by bundled hooks/skills.

Bundled skills must not perform free-form text edits against
`execution.yaml`. The state command validates the schema, expected revision,
active loop ID, and transition before using the same Rust persistence layer as
the UI.

Minimum command surface:

```text
loopdeck-state show
loopdeck-state promote <loop-id>
loopdeck-state complete <loop-id> [--commit <sha>]
loopdeck-state abandon <loop-id> --reason <text>
```

The implementation may expose this through the LoopDeck binary, a Tauri sidecar
entry point, or another narrow local interface. It must not duplicate parsing
and transition rules in shell scripts.

## Migration and Compatibility

### Detection

On project open:

- `execution.yaml` exists → use structured state.
- Only `loops.md` exists → continue legacy read mode and offer migration.
- Neither exists → create an empty `execution.yaml`.
- Both exist → `execution.yaml` is authoritative; show `loops.md` as a legacy
  artifact unless it is the migration snapshot.

### Migration flow

1. Parse `loops.md` with the existing lenient parser.
2. Map Current and History entries to PRD items by exact epic + PRD + title.
3. Assign the stable PRD ID when exactly one match exists.
4. Mark ambiguous or missing matches as `legacy/<generated-id>` with
   `migration_status: unmatched`; never fuzzy-match.
5. Convert Next Steps into queue entries only when they carry an unambiguous
   loop ID or origin reference. Preserve all other checklist text in the
   migration report.
6. Preview the resulting state and all warnings.
7. On confirmation, atomically write `execution.yaml` and copy the original to
   `.loopdeck/loops.legacy.md`.
8. Leave the backup untouched throughout 0.2.x.

Migration is explicit and idempotent. Cancelling leaves the project in legacy
mode with no writes.

### Legacy `loops.md`

LoopDeck supports legacy reads throughout 0.2.x. New structured-state projects
do not continuously generate `loops.md`, because a generated mutable-looking
file would recreate the duplicate-authority problem.

An explicit **Export execution summary as Markdown** action may create a
snapshot for sharing. The export carries a generated-file warning and is never
read back as state.

## UX Requirements

### Epics and PRDs

- Render each loop's derived state beside the original Markdown checklist.
- Keep the original checkbox visible as authored text, but do not equate it
  with execution completion.
- Explain discrepancies, such as a checked planned item with no History record.
- Promote by stable ID.
- Show unmatched legacy records in a reconciliation panel.

### Current loop

- Show ID, origin, start time, and revision-backed action state.
- Reject stale Complete/Abandon actions and refresh rather than overwriting.
- Preserve the current clobber guard.

### Progress language

Use precise terms:

- **Planned**: exists in the spec.
- **In progress**: active execution exists.
- **Implemented**: execution completed.
- **Committed**: Git commit evidence exists.
- **In review**: optional remote PR is open.
- **Shipped**: merge or release evidence exists.

Do not label an epic "done" when LoopDeck can only prove implementation.

## Phases

### Phase 1 — Stable IDs in the spec layer

- [ ] `structured-state/parse-loop-id` Extend `PrdLoop` with a stable ID parsed separately from its title
- [ ] `structured-state/validate-loop-ids` Detect duplicate and malformed IDs with file-and-line diagnostics
- [ ] `structured-state/author-loop-ids` Update `loopdeck-epic-author` to generate stable IDs
- [ ] `structured-state/render-legacy-items` Render legacy ID-less items but disable Promote with a remediation
- [ ] `structured-state/update-dogfood-prds` Add IDs to LoopDeck's existing active PRD checklist items

### Phase 2 — Execution schema and persistence

- [ ] `structured-state/execution-types` Define versioned Rust types for current, queue, origin, history, outcome, and Git evidence
- [ ] `structured-state/execution-validation` Validate schema version, uniqueness, transitions, and expected revision
- [ ] `structured-state/execution-persistence` Load, back up, and atomically write `.loopdeck/execution.yaml`
- [ ] `structured-state/execution-recovery` Add malformed-primary recovery without destructive automatic restoration
- [ ] `structured-state/execution-tests` Cover round-trip, stale revision, duplicate ID, unknown version, backup recovery, and atomic completion

### Phase 3 — State transitions and agent integration

- [ ] `structured-state/promote-by-id` Replace title-based promotion with a validated stable-ID transition
- [ ] `structured-state/complete-loop` Implement atomic current-to-history completion
- [ ] `structured-state/abandon-loop` Implement abandonment with a required reason
- [ ] `structured-state/state-command` Provide the narrow LoopDeck-owned state command for hooks and skills
- [ ] `structured-state/update-memory-skill` Update `loopdeck-memory` and loop-running skills to use validated transitions
- [ ] `structured-state/remove-freeform-runtime-edits` Remove bundled instructions that directly edit runtime Markdown

### Phase 4 — Migration and compatibility

- [x] `structured-state/legacy-reader` Preserve legacy `loops.md` read mode through 0.2.x
- [x] `structured-state/migration-preview` Build exact-match migration with warnings and no fuzzy matching
- [x] `structured-state/migration-confirmation` Require explicit confirmation before writing structured state
- [x] `structured-state/migration-backup` Preserve `loops.legacy.md` and make migration idempotent
- [x] `structured-state/migration-ui` Add a project-level migration and reconciliation surface
- [x] `structured-state/migration-tests` Cover empty, current-only, history, ambiguous-title, missing-origin, both-files, and cancelled migration cases

### Phase 5 — Derived progress UI

- [x] `structured-state/execution-index` Join PRD loops to execution records exclusively by stable ID
- [x] `structured-state/derived-loop-status` Render planned, queued, in-progress, completed, abandoned, and unmatched states
- [x] `structured-state/derived-prd-progress` Derive PRD progress from required child loop IDs
- [x] `structured-state/derived-epic-progress` Derive epic progress from its PRDs without trusting manual completion frontmatter
- [x] `structured-state/status-discrepancies` Surface authored-checkbox and derived-state disagreement without rewriting the PRD
- [x] `structured-state/markdown-export` Add an explicit, non-authoritative Markdown execution-summary export

### Phase 6 — Git delivery evidence

- [x] `structured-state/capture-commit` Allow completion to record a verified local commit SHA
- [x] `structured-state/validate-commit` Distinguish missing, reachable, and unreachable local commit evidence
- [x] `structured-state/derive-delivery` Render implemented versus committed without requiring network access
- [x] `structured-state/provider-boundary` Define an optional provider interface for in-review and shipped enrichment

## Acceptance Criteria

1. Renaming a PRD loop title after promotion does not break its execution
   relationship or completion display.
2. Completing a loop is one atomic write that clears Current and appends
   History; a crash cannot leave both or neither transition applied.
3. A stale UI revision cannot overwrite a newer execution state.
4. PRD and epic progress is derived from stable IDs and execution records, not
   checkbox state, frontmatter completion, or title matching.
5. A project with only `loops.md` continues to work without migration.
6. Migration shows every unmatched or ambiguous record and performs no fuzzy
   identity guesses.
7. Cancelling migration produces no filesystem changes.
8. Confirmed migration preserves the original file as
   `.loopdeck/loops.legacy.md`.
9. A newer unsupported schema is never rewritten by an older LoopDeck.
10. Bundled skills and hooks do not directly rewrite execution state as
    unvalidated Markdown or YAML text.
11. LoopDeck remains fully usable offline; remote shipping data is optional.
12. Epics, PRDs, and decisions remain readable and authorable as Markdown.

## Success Metrics

During dogfood across at least three repositories:

- Zero title-drift failures after promotion.
- Zero silent clobbers from stale UI actions.
- Every legacy record is either mapped exactly or visibly marked unmatched.
- No manual PRD checkbox update is required for the UI to show execution
  completion.
- A user can explain the difference between Planned, Implemented, and Shipped
  from the UI labels without consulting documentation.
- Migration can be completed or cancelled in under two minutes for a typical
  project.

## Risks

| Risk | Mitigation |
|---|---|
| YAML becomes another hand-edited database | Route bundled automation through a validated LoopDeck-owned command; keep the file inspectable but app-managed |
| Stable IDs add authoring ceremony | Generate them in `loopdeck-epic-author`; suggest IDs for legacy items and require confirmation |
| Migration loses prose or unusual checklist layouts | Preserve the original file, preview all mappings, and carry unmatched text into the report |
| One YAML file grows with long history | Measure real history size; introduce a checkpointed archive only in a later schema version |
| Git-derived shipping state becomes provider-specific | Keep local commit evidence in the core schema and remote review/merge data behind an optional interface |
| Derived progress surprises users whose boxes are checked manually | Show authored and derived disagreement explicitly; never silently rewrite the spec |
| Agent completion cannot reach Tauri IPC directly | Ship a narrow local state command backed by the same Rust transition implementation |

## Open Questions

- Should retry attempts reuse the same loop ID with an `attempt` counter, or
  require a new ID? **Lean:** same logical ID plus monotonically increasing
  `attempt`; progress reflects success if any non-superseded attempt completes.
- Should queue ordering live in the spec or execution state? **Lean:** execution
  state. PRD order communicates the plan; queue order communicates the user's
  current scheduling decision.
- Should completed history remain in `execution.yaml` indefinitely?
  **Lean:** yes for schema v1. Revisit only after measured repositories exceed
  a practical size threshold.
- Should the app remove legacy `status` from epic/PRD frontmatter?
  **Lean:** no automatic rewrite. Introduce `lifecycle` in a separate format
  migration after derived progress is proven.
- What exact binary surface should expose `loopdeck-state`?
  **Lean:** reuse the Rust domain and persistence code in a narrow subcommand;
  do not implement state transitions in Bash or duplicate them in TypeScript.
