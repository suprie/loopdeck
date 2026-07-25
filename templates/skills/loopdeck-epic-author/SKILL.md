---
name: loopdeck:epic-author
description: Draft a new epic or PRD in the docs/epics/ spec format. Use when the user wants to "draft a new epic", "structure this feature as a spec", "elaborate this goal into a PRD", or turns a coarse one-paragraph intent into a reviewable Epic → PRD → Phase → Loop plan. Asks a fixed, small set of clarifying questions whose answers ARE the spec, then writes reviewable drafts to docs/epics/. Drafts only — never commits, never promotes a loop, never touches .loopdeck/loops.md.
argument-hint: "[optional epic slug or one-paragraph intent]"
allowed-tools: [Read, Write, Glob, Grep, Bash]
---

# Epic Author — Drafting Aid for the Spec Layer

`loopdeck:epic-author` turns a user's coarse intent ("I want the agent to ship
features end-to-end without me approving every command") into the
`docs/epics/<slug>/` spec format that `epic.rs` parses and the Epics view
renders. It is **on-demand and human-invoked**: the user invokes it and answers
a small set of clarifying questions; the skill drafts an epic README + one or
more PRDs and writes them to disk for review.

This skill is **mechanics only — it drafts, the human decides.** It produces
**reviewable drafts** with `status: proposed`. It never commits, never
promotes a loop, never touches `.loopdeck/loops.md`. The spec layer (`docs/`)
is intention owned by the human; the runtime layer (`.loopdeck/`) is execution
owned by the app/agent.

> Milestone boundary: in 0.2.0 this skill is human-invoked
> (`/loopdeck:epic-author`). The 0.3.0 app-invoked PRD-dialogue front-end (a UI
> button that launches the same dialogue into a chat surface) is separate; the
> skill's behavior is unchanged across both.

## Posture Rules (load-bearing)

These rules keep the skill from recreating the autonomous-planning drift the
`support-project-management` epic exists to kill. They are not optional.

1. **On-demand only.** Fire when the user invokes `/loopdeck:epic-author` or
   uses a natural-language trigger like "draft a new epic", "structure this
   feature", "elaborate this goal into a spec". Never self-fire, never watch
   for "the roadmap seems empty." The trigger phrasing above is on-purpose.
2. **Drafts, not decisions.** Write files with `status: proposed`. Never `git
   commit`, never push, never promote a loop, never touch `.loopdeck/loops.md`.
   The human owns all of those.
3. **Fixed question set.** Ask the five (optionally six) questions below — not
   an open-ended dialogue. Open-ended dialogue is how the old orchestrator
   drifted into re-decomposing plans.
4. **Confirm before writing.** Show the user the full epic→PRD→phase→loop tree
   and get confirmation before writing any PRD file. The epic README may be
   written in Step 1, but the user can abort before Step 5 writes the PRDs.
5. **Refuse to overwrite.** Never overwrite an existing epic README or PRD
   without explicit per-file confirmation. On a slug collision, abort with a
   remediation hint (rename the slug, or use Q6 to draft a PRD into the existing
   epic).
6. **No sub-agents.** `allowed-tools` omits `Agent`/`TaskCreate`. Run in the
   main conversation so the user sees every question and every drafted section.
7. **Read before write.** Before drafting, read existing `docs/epics/` to avoid
   slug/title collisions and to suggest continuations ("you have an in-flight
   0.2.0 epic — is this a new PRD in it?").

## Step 0 — Read existing epics

`ls docs/epics/` (Bash) and read each `README.md`'s frontmatter. Note existing
slugs, titles, milestones, and statuses. This drives Q6 (new epic vs. new PRD
in an existing one) and prevents collisions.

## The Clarifying-Question Set

Ask a **fixed, small set** of questions. The questions are chosen so their
answers **are the substance of the spec** — the skill *places* the answers into
the format, it does not *generate* the answers. Ask them grouped (Q1–Q2, then
Q3, then Q4–Q6), confirming before proceeding.

### Q1 — User-visible outcome
> "When this is done, what can a user do that they couldn't before? One
> sentence, from the user's perspective (not the implementation's)."

Becomes the epic `description` (folded scalar) and seeds `## Motivation`.
**Reject implementation-shaped answers** ("add a FullAccess variant to
PermissionMode") in favor of outcome-shaped ones ("the agent runs a whole
feature without per-command approval prompts"). Ask once more if the first
answer is implementation-shaped.

### Q2 — Success criteria
> "How will you know it's done? List 2–5 testable outcomes."

Become `## Success Criteria` verbatim (lightly edited for "criterion can be
checked" voice). They must be **checkable, not aspirational** — they seed the
acceptance criteria `loopdeck:prd-verifier` checks later.

### Q3 — In scope vs. out of scope
> "What's in scope, and just as importantly, what's explicitly out of scope?
> Name 3–8 in-scope items and 2–5 out-of-scope items."

In-scope items seed `## Scope` and drive phase decomposition (each maps to one
or more phases). Out-of-scope items seed `## Non-Goals` verbatim. **Push back**
on items that are too large ("build the whole backend" → "which slice?") or too
vague ("improve UX" → "improve what, for whom, measured how?").

### Q4 — Riskiest unknown
> "What's the single thing you're most unsure about — the unknown that, if it
> goes wrong, derails this?"

Becomes the first row of `## Risks` with a mitigation the skill drafts and the
user edits. If this unknown is load-bearing, propose a **spike/exploration phase
first** (Step 3).

### Q5 — Target milestone + owner
> "Which milestone does this target? (e.g. 0.2.0, 0.3.0, unassigned) And who
> owns it?"

Becomes frontmatter `milestone` (the skill quotes it in the output to dodge
YAML float parsing — `0.2.0` unquoted parses as `0.2`) and `owner`.

### Q6 (optional) — Existing epic?
> "Is this a new epic, or a new PRD inside an existing epic? If existing, which
> slug?"

If existing, read `docs/epics/<slug>/README.md` to inherit milestone/owner,
draft a single PRD, and update the epic README's `## PRD Index` table. If the
slug doesn't exist, abort with a remediation hint.

## Decomposition Flow: Goal → Epic → PRD → Phase → Loop

```
user intent
    │
    ▼  (Q1–Q5 answered)
epic README  ─── docs/epics/<slug>/README.md
    │              frontmatter: title, slug, milestone, status, owner, description
    │              body: Motivation, Scope, Non-Goals, PRD Index, ADRs, Success Criteria, Risks
    │
    ▼  (per in-scope bucket)
PRD  ──────── docs/epics/<slug>/prd-<topic>.md
    │              frontmatter: prd, epic, milestone, status, description
    │              body: Overview, Problem Statement, Goals, Non-Goals, Design, Phases, Open Questions
    │
    ▼  (per cohesive unit of work)
Phase  ─────── "### Phase N — <Name>"
    │              a GFM checklist
    │
    ▼  (atomic, promote-able)
Loop  ──────── "- [ ] <loop title>"
                   becomes loops.md ## Current on promote
```

### Step 1 — Draft the epic README
From Q1–Q5, draft `docs/epics/<slug>/README.md`:
- **Frontmatter:** `title`, `slug` (kebab-case, MUST match the directory name),
  `milestone` (quoted), `status: proposed`, `started: <today>`, `owner`,
  `description` (folded scalar from Q1).
- **Body:** `## Motivation` (Q1 + outcome framing), `## Scope` (Q3 in-scope as
  bullets, out-of-scope noted), `## Non-Goals` (Q3 out-of-scope verbatim),
  `## PRD Index` (filled in Step 2), `## Architecture Decisions` (a single
  placeholder ADR the user fills in — `### ADR-1: <title> — fill in`), `##
  Success Criteria` (Q2 verbatim), `## Risks` (Q4 + drafted mitigation).

Write the README and show it to the user for confirmation before PRD
decomposition.

### Step 2 — Decompose into PRDs
Each in-scope bucket from Q3 is a PRD candidate. Propose the PRD list (filename
+ one-line description each) and ask the user to confirm/merge/split. Rule of
thumb: **one PRD per cohesive deliverable that could ship independently**.
Tightly-coupled buckets ("backend + frontend for the same feature") may belong
in one PRD with two phases.

For each confirmed PRD, draft `docs/epics/<slug>/prd-<topic>.md`:
- **Frontmatter:** `prd` (filename without `.md`), `epic` (the slug),
  `milestone` (inherited, quoted), `status: proposed`, `description` (folded
  scalar).
- **Body:** `## Overview`, `## Problem Statement`, `## Goals` (P0/P1/P2 priority
  table), `## Non-Goals`, `## Design` (a stub — the human fills it in; the skill
  does not bake in architectural decisions), `## Phases`, `## Open Questions`.

### Step 3 — Decompose PRDs into phases
Propose a phase list per PRD. Mechanical rules:
- **One phase per cohesive unit of work that produces a reviewable checkpoint**
  ("Backend data model", "IPC layer", "frontend selector", "tests" is a typical
  4-phase shape).
- **A spike/exploration phase comes first if Q4's riskiest unknown is
  load-bearing.** Propose it explicitly; the user can drop it.
- **A tests/verification phase comes last** unless the PRD is pure-spec.

The user confirms/edits the phase list per PRD before checklists are filled.

### Step 4 — Decompose phases into loops
For each phase, draft the GFM checklist. **Each `- [ ]` item must be:**
1. **Atomic** — completable in a single agent loop, not a multi-session epic.
2. **Single-loop-shaped** — fits the `**Goal**` field of `loops.md ## Current`
   (one sentence, verb-led, checkable). The promote-to-loop action takes the
   item text **verbatim** as the promoted loop's `**Goal**` — so the item text
   *is* the loop goal.
3. **Ordered within the phase** — earlier items unblock later ones where there's
   a dependency; otherwise logical.

If an item can't be expressed as a single loop ("implement the whole IPC
layer"), split it. Propose the split and ask for confirmation.

### Step 5 — Final review + write
Print the full tree (epic → PRDs → phases → loops) and ask the user to confirm
before writing any PRD file. **Files are written only on confirmation.** Then
report:
- Files created (paths).
- A reminder that the epic is `status: proposed` and **not committed** — the
  user reviews, edits, and `git add`s.
- A pointer to the Epics view (`/epics`) where the new epic appears once
  `epic.rs` parses it.

## Format Contract (must parse cleanly in `epic.rs`)

The skill MUST produce files that `epic.rs` parses cleanly. Load-bearing
invariants:

- **Epic frontmatter required fields:** `title`, `slug`, `milestone`, `status`,
  `description`. `slug` MUST equal the directory name. `milestone` MUST be quoted
  (YAML float trap — `0.2.0` → `0.2`).
- **PRD frontmatter required fields:** `prd`, `epic`, `status`, `description`.
  `prd` MUST equal the filename without `.md`. `epic` MUST equal the parent
  epic's `slug`.
- **Phase heading shape:** `### Phase N — <Name>` (em dash, not hyphen — matches
  the existing epics). Followed by a GFM checklist.
- **Status vocabularies:** epic `status` ∈ {proposed, in_progress, completed,
  abandoned}; PRD `status` ∈ {proposed, accepted, completed}.

The skill follows the contract as documented; it does not embed a copy of the
parser. If `epic.rs` later adds strict validation, the skill's output must still
pass.

## Open Defaults (resolved)

- **Slug derivation:** derive from Q1 (kebab-case of the outcome), show the user,
  let them edit. Auto-derivation removes one question; the user confirms in
  Step 5 anyway.
- **How many PRDs to propose:** group coupled items per the "one PRD per
  independent deliverable" rule; let the user split in Step 2. Start coarser;
  splitting is cheaper than merging.
- **`## Design` section:** leave a stub in 0.2.0. The human fills it in once they
  start working; the skill drafting a Design sketch risks baking in
  architectural decisions before the human has made them.
- **ADRs:** no — leave a single placeholder ADR so the user knows the shape.
  ADRs capture decisions; the skill hasn't made any yet.
- **Updating `## PRD Index` when adding a PRD to an existing epic:** match the
  existing table's column shape exactly; if the table is unparseable, append the
  row at the end with a comment. Never rewrite a human-authored table.
