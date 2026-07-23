---
prd: prd-epic-author
epic: support-project-management
milestone: "0.2.0"
status: proposed
description: >
  Specify the loopdeck-epic-author skill in detail: an on-demand drafting aid
  that elaborates a user's coarse intent into the docs/epics/ spec format via
  clarifying questions. Produces reviewable epic README + PRD drafts that
  match the prd-spec-layer format contract, with phases decomposed into
  promote-able checklist loops. Does not commit, does not promote, does not
  touch loops.md. Lands as a human-invoked skill in 0.2.0; the app-invoked
  PRD-dialogue front-end that launches it from a button is 0.3.0.
---

# PRD — Epic Author Skill (loopdeck-epic-author)

## Overview

`loopdeck-epic-author` is the skill that turns a user's one-paragraph intent
("I want the agent to ship features end-to-end without me approving every
command") into the `docs/epics/<slug>/` spec format that `epic.rs` parses and
the Epics view renders. It is **on-demand and human-invoked** — the user
invokes `/loopdeck:epic-author` and answers a small set of clarifying
questions; the skill drafts an epic README + one or more PRDs and writes them
to disk for the user to review and edit.

This PRD fills the detail gap left by `prd-skill-split.md`, which names the
skill (P0 goal at `prd-skill-split.md:53`) and lists it in the split's
manifest (`:129`) but does not specify its internals: the clarifying-question
set, the goal→epic→PRD→phase→loop decomposition flow, the format contract
with `prd-spec-layer.md`, or the posture rules that keep it from drifting
into autonomous planning.

The skill is mechanics only — it drafts, the human decides. It produces
**reviewable drafts**, never commits, never promotes loops, never touches
`.loopdeck/loops.md`. This mirrors ADR-1 of this epic: the spec layer
(`docs/`) is intention, owned by the human; the runtime layer
(`.loopdeck/`) is execution, owned by the app/agent.

**Milestone boundary (per `support-project-management/README.md:59-61`):**

- **0.2.0 (this PRD)** — the skill itself. Human-invoked via
  `/loopdeck:epic-author`. Writes files under `docs/epics/`.
- **0.3.0** — the app-invoked PRD-dialogue front-end. LoopDeck launches the
  dialogue from a UI button (e.g. "Draft a new epic…"), streams the
  clarifying-question exchange into a chat surface, and writes the draft.
  The skill's behavior is unchanged; only the invocation surface changes.

## Problem Statement

Today the only way to create an epic is to hand-author the README + PRDs
following the `prd-spec-layer.md` format contract. That format is precise
(YAML frontmatter, `## Phases` with `### Phase N — Name` headings + GFM
checklists, milestone-as-quoted-string to dodge YAML float parsing, etc.),
and hand-authoring it cold is friction — the user has to hold the format
rules and the decomposition strategy in their head at the same time.

The friction matters because the spec layer is the load-bearing artifact of
the whole epic: `epic.rs` parses it, the Epics view renders it, and the
promote-to-loop bridge acts on its checklists. If authoring is painful, users
won't use the spec layer, and the runtime-layer (`.loopdeck/loops.md`)
reverts to the flat-list model this epic exists to escape.

A drafting aid closes the gap: the user states the intent, answers a handful
of questions whose answers *are* the substance of the spec (outcome, scope,
non-goals, riskiest unknown), and receives a draft that already conforms to
the format contract. The user reviews, edits, commits. The skill does the
clerical work; the human owns the decisions.

The skill is explicitly **not** an autonomous planner. The
`support-project-management` epic's ADR on splitting the orchestrator
(`prd-skill-split.md:36-46`) exists precisely because the old orchestrator
conflated mechanics with strategy and the agent kept re-decomposing plans the
human had already authored. `loopdeck-epic-author` must not recreate that
drift: it drafts when asked, produces a reviewable artifact, and stops.

## Goals

| Priority | Goal |
|--------|------|
| P0 | `.agents/skills/loopdeck-epic-author/SKILL.md` exists with the correct frontmatter (`name`, trigger-rich `description`, `argument-hint`, `allowed-tools`) |
| P0 | The skill's clarifying-question set elicits: user-visible outcome, success criteria, in-scope vs out-of-scope, riskiest unknown, target milestone, owner |
| P0 | The skill drafts an epic README at `docs/epics/<slug>/README.md` matching the frontmatter schema + body sections defined in `prd-spec-layer.md` |
| P0 | The skill drafts one or more PRDs at `docs/epics/<slug>/prd-<topic>.md` matching the PRD frontmatter schema + body sections, with `## Phases` decomposed into `### Phase N — Name` + GFM checklists |
| P0 | Each unchecked checklist item is a promote-able loop (atomic, single-loop-shaped, matches the `prd-epics-view.md` Promote Contract input) |
| P0 | Posture rules in the skill body: never `git commit`, never promote, never touch `.loopdeck/loops.md`, never spawn sub-agents, produce drafts only |
| P0 | Trigger phrasing is on-demand ("when the user wants to draft / structure a new epic"), not a standing intent ("help plan the roadmap") |
| P1 | The skill reads existing `docs/epics/` before drafting to avoid slug/title collisions and to suggest continuations |
| P1 | The skill refuses to overwrite an existing epic README or PRD without explicit confirmation |
| P1 | The skill surfaces the format contract inline (frontmatter fields, phase heading shape) so the draft is self-documenting for a user editing it later |
| P2 | The skill can draft a single PRD into an existing epic (not just whole epics) for the "add a feature to an in-flight epic" case |

## Non-Goals

- **Autonomous planning.** The skill drafts when invoked; it does not watch
  for "the roadmap seems empty" and self-fire. The 0.3.0 app-invoked
  front-end is where structured triggering lands, and even there the user
  clicks the button.
- **Committing or promoting.** Drafts land on disk for the human to `git
  add` and review. The promote-to-loop bridge (`prd-epics-view.md`) is a
  separate, later, human-triggered action.
- **AI phase decomposition heuristics.** The skill decomposes into phases
  using the user's own answers (scope buckets → phases), not by guessing
  architecture. If the user's answers don't yield a clean decomposition, the
  skill says so and asks — it does not invent phases.
- **Editing existing epics/PRDs.** The skill drafts new files. Editing is
  done in the user's editor. (The P2 "draft a single PRD into an existing
  epic" goal is the closest exception, and it only adds a file, never
  rewrites one.)
- **The app-invoked PRD-dialogue front-end.** That is 0.3.0. This PRD ships
  the skill; the 0.3.0 front-end wraps the same skill in a UI surface.
- **Validating the draft against `epic.rs`.** The skill follows the format
  contract, and `epic.rs` parses strictly — a mismatch surfaces as a parse
  error in the Epics view, which is the right place to catch it. The skill
  does not embed a copy of the parser.

## The Clarifying-Question Set

The skill asks a **fixed, small set** of questions — not an open-ended
dialogue. Open-ended dialogue is what recreates the autonomous-planning drift
this epic exists to kill. The questions are chosen so their answers are the
substance of the spec; the skill's job is to *place* the answers into the
format, not to *generate* the answers.

### Question 1 — User-visible outcome

> "When this is done, what can a user do that they couldn't before? One
> sentence, written from the user's perspective (not the implementation's)."

This becomes the epic `description` (folded scalar) and seeds the
`## Motivation`. Reject implementation-shaped answers ("add a FullAccess
variant to PermissionMode") in favor of outcome-shaped ones ("the agent runs
a whole feature without per-command approval prompts"). Ask once more if the
first answer is implementation-shaped.

### Question 2 — Success criteria

> "How will you know it's done? List 2–5 testable outcomes."

These become the `## Success Criteria` bullets verbatim (lightly edited for
the "criterion can be checked" voice). They also seed the per-PRD acceptance
criteria that `loopdeck:prd-verifier` (0.3.0, `agent-full-access` epic) will
check against later — so the criteria must be checkable, not aspirational.

### Question 3 — In scope vs out of scope

> "What's in scope, and just as importantly, what's explicitly out of scope?
> Name 3–8 in-scope items and 2–5 out-of-scope items."

In-scope items seed `## Scope` (in scope) and drive the phase decomposition
(each in-scope item maps to one or more phases). Out-of-scope items seed
`## Non-Goals` verbatim. The skill pushes back on items that are too large
("build the whole backend" → "which slice of the backend?") or too vague
("improve UX" → "improve what, for whom, measured how?").

### Question 4 — Riskiest unknown

> "What's the single thing you're most unsure about — the unknown that, if it
> goes wrong, derails this?"

This becomes the first row of `## Risks` with a mitigation the skill drafts
and the user edits. Surfacing the riskiest unknown *before* decomposition is
what lets the skill propose a spike/exploration phase first if the unknown is
load-bearing.

### Question 5 — Target milestone + owner

> "Which milestone does this target? (e.g. 0.2.0, 0.3.0, unassigned) And who
> owns it?"

These become frontmatter `milestone` (quoted in the output to dodge YAML
float parsing — the skill does this mechanically) and `owner`.

### Optional Question 6 — Existing epic?

> "Is this a new epic, or a new PRD inside an existing epic? If existing,
> which slug?"

If existing, the skill reads `docs/epics/<slug>/README.md` to inherit
milestone/owner, drafts a single PRD, and updates the epic README's
`## PRD Index` table to include the new PRD. If the slug doesn't exist, abort
with a remediation hint.

## Decomposition Flow: Goal → Epic → PRD → Phase → Loop

The skill decomposes top-down, at each step asking the user to confirm before
proceeding to the next level. The shape mirrors the spec layer exactly:

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
                   becomes loops.md ## Current on promote (prd-epics-view.md Promote Contract)
```

### Step 1 — Draft the epic README

From Q1–Q5, draft `docs/epics/<slug>/README.md`:

- **Frontmatter** exactly per `prd-spec-layer.md:78-90`: `title`, `slug`
  (kebab-case, MUST match the directory name), `milestone` (quoted),
  `status: proposed`, `started: <today>`, `owner`, `description` (folded
  scalar from Q1).
- **Body sections** per `prd-spec-layer.md:117-119`: `## Motivation` (from
  Q1 + the outcome framing), `## Scope` (Q3 in-scope as bullets, out-of-scope
  noted), `## Non-Goals` (Q3 out-of-scope verbatim), `## PRD Index` (table —
  filled in Step 2 as PRDs are drafted), `## Architecture Decisions`
  (left as a stub with one placeholder ADR the user fills in), `## Success
  Criteria` (Q2 verbatim), `## Risks` (Q4 + drafted mitigation).

The skill writes the README to disk and shows it to the user for confirmation
before proceeding to PRD decomposition.

### Step 2 — Decompose into PRDs

Each in-scope bucket from Q3 becomes a PRD candidate. The skill proposes the
PRD list (filename + one-line description each) and asks the user to
confirm/merge/split. Rule of thumb: **one PRD per cohesive deliverable that
could ship independently**. If two buckets are tightly coupled ("backend +
frontend for the same feature"), they may belong in one PRD with two phases.

For each confirmed PRD, draft `docs/epics/<slug>/prd-<topic>.md`:

- **Frontmatter** per `prd-spec-layer.md:100-109`: `prd` (filename without
  `.md`), `epic` (the slug), `milestone` (inherited, quoted), `status:
  proposed`, `description` (folded scalar).
- **Body** per `prd-spec-layer.md:121-123`: `## Overview`, `## Problem
  Statement`, `## Goals` (P0/P1/P2 priority table), `## Non-Goals`,
  `## Design` (stub — the user fills in during implementation, or the skill
  drafts a sketch from the in-scope items if they're concrete enough),
  `## Phases`, `## Open Questions`.

### Step 3 — Decompose PRDs into phases

For each PRD, propose a phase list. The decomposition rule is mechanical:

- **One phase per cohesive unit of work that produces a reviewable
  checkpoint.** "Backend data model," "IPC layer," "frontend selector,"
  "tests" is a typical 4-phase shape.
- **A spike/exploration phase comes first if Q4's riskiest unknown is
  load-bearing.** The skill proposes this explicitly; the user can drop it.
- **A tests/verification phase comes last** unless the PRD is pure-spec.

The user confirms/edits the phase list per PRD before the skill fills in
checklists.

### Step 4 — Decompose phases into loops

For each phase, draft the GFM checklist. **Each `- [ ]` item must be:**

1. **Atomic** — completable in a single agent loop, not a multi-session epic.
2. **Single-loop-shaped** — fits the `**Goal**` field of `loops.md ## Current`
   (one sentence, verb-led, checkable). The `prd-epics-view.md` Promote
   Contract (`prd-epics-view.md:60-90`) takes the item text verbatim as the
   promoted loop's `**Goal**` — so the item text *is* the loop goal.
3. **Ordered within the phase** — earlier items unblock later ones where
   there's a dependency; otherwise alphabetical/logical.

If an item can't be expressed as a single loop ("implement the whole IPC
layer"), split it. The skill proposes the split and asks for confirmation.

### Step 5 — Final review

The skill prints the full tree (epic → PRDs → phases → loops) and asks the
user to confirm before writing any PRD files. Files are written only on
confirmation. The skill then reports:

- Files created (paths).
- A reminder that the epic is `status: proposed` and not committed — the
  user reviews, edits, and `git add`s.
- A pointer to the Epics view (`/epics`) where the new epic will appear once
  `epic.rs` parses it.

## Format Contract with `prd-spec-layer.md`

The skill MUST produce files that `epic.rs` parses cleanly. The contract is
defined by `prd-spec-layer.md`; this PRD does not restate it in full, only
the load-bearing invariants the skill must enforce:

- **Epic frontmatter required fields:** `title`, `slug`, `milestone`,
  `status`, `description`. `slug` MUST equal the directory name. `milestone`
  MUST be quoted (YAML float trap).
- **PRD frontmatter required fields:** `prd`, `epic`, `status`,
  `description`. `prd` MUST equal the filename without `.md`. `epic` MUST
  equal the parent epic's `slug`.
- **Phase heading shape:** `### Phase N — <Name>` (em dash, not hyphen —
  matches the existing epics). Followed by a GFM checklist.
- **Status vocabularies:** epic `status` ∈ {proposed, in_progress,
  completed, abandoned}; PRD `status` ∈ {proposed, accepted, completed}.

If `epic.rs` later adds strict validation (currently lenient on body, strict
on frontmatter per `prd-spec-layer.md:51`), the skill's output must still
pass. The skill follows the contract as documented; it does not embed a copy
of the parser.

## Posture Rules (load-bearing)

These are the rules that keep the skill from recreating the
autonomous-planning drift the epic exists to kill. They live in the skill's
body, not just this PRD.

1. **On-demand only.** The skill fires when the user invokes it
   (`/loopdeck:epic-author` or natural-language triggers like "draft a new
   epic", "structure this feature as a spec"). It never self-fires, never
   watches for "the roadmap seems empty." The trigger phrasing in the
   frontmatter `description` must be on-demand ("when the user wants to
   draft"), not standing intent ("help plan the roadmap").
2. **Drafts, not decisions.** The skill writes files with `status: proposed`.
   It never commits, never pushes, never promotes a loop, never touches
   `.loopdeck/loops.md`. The human owns all of those actions.
3. **Fixed question set.** The clarifying questions are the five (optionally
   six) in this PRD — not an open-ended dialogue. Open-ended dialogue is how
   the old orchestrator drifted into re-decomposing plans.
4. **Confirm before writing.** The skill shows the user the full
   epic→PRD→phase→loop tree and gets confirmation before writing any PRD
   file. The epic README is written in Step 1 but the user can abort before
   Step 5 writes the PRDs.
5. **Refuse to overwrite.** The skill never overwrites an existing epic
   README or PRD without explicit per-file confirmation. If a slug
   collision is detected, it aborts with a remediation hint (rename the
   slug, or use Q6 to draft a PRD into the existing epic).
6. **No sub-agents.** `allowed-tools` omits `Agent`/`TaskCreate`. The skill
   runs in the main conversation; the user sees every question and every
   drafted section. This keeps the drafting transparent and reviewable.
7. **Read before write.** Before drafting, the skill reads existing
   `docs/epics/` to avoid collisions and to suggest continuations (e.g.
   "you have an in-flight 0.2.0 epic — is this a new PRD in it?").

## Phases

### Phase 1 — Skill scaffold

- [ ] Create `.agents/skills/loopdeck-epic-author/SKILL.md`
- [ ] Frontmatter: `name: loopdeck:epic-author`, trigger-rich `description` (on-demand phrasing — "Use when the user wants to draft / structure a new epic or PRD…"), `argument-hint: [optional epic slug or intent]`, `allowed-tools: [Read, Write, Glob, Grep, Bash]`
- [ ] `Bash` is included only for `git status`/directory checks; `Write` is the file-creation tool. No `Edit` (the skill writes new files, doesn't edit existing ones), no `Agent`/`TaskCreate` (posture rule 6)

### Phase 2 — Clarifying-question flow

- [ ] Document the five fixed questions + optional sixth in the skill body, with the rejection rules (Q1 implementation-shaped → ask once more; Q3 too-large/too-vague → push back)
- [ ] Document the "answers are the substance" framing — the skill places answers into the format, does not generate them
- [ ] Document the confirm-before-proceeding gate between each question group

### Phase 3 — Decomposition flow

- [ ] Document Step 1 (epic README draft) with the frontmatter + body mapping per `prd-spec-layer.md`
- [ ] Document Step 2 (PRD decomposition) with the one-PRD-per-independent-deliverable rule and the merge/split guidance
- [ ] Document Step 3 (phase decomposition) with the spike-first-if-riskiest-unknown-load-bearing rule and the tests-last convention
- [ ] Document Step 4 (loop decomposition) with the three loop invariants (atomic, single-loop-shaped, ordered) and the split-if-not-single-loop rule
- [ ] Document Step 5 (final review + write-on-confirmation + status:proposed reminder)

### Phase 4 — Format contract + posture rules

- [ ] Embed the format-contract invariants (required frontmatter fields, slug=dirname, milestone quoted, phase heading shape, status vocabularies) in the skill body
- [ ] Embed the seven posture rules in the skill body
- [ ] Document the read-before-write rule and the refuse-to-overwrite behavior

### Phase 5 — Integration with the spec layer

- [ ] Verify drafted files parse cleanly via `epic.rs` (dogfood: draft a trivial epic, confirm it appears in `/epics`)
- [ ] Verify drafted checklist items are promote-able via the `prd-epics-view.md` Promote Contract (item text fits `**Goal**`, promotes without clobbering)
- [ ] Verify the `## PRD Index` table in the epic README is updated when PRDs are drafted in Step 2
- [ ] Update the managed-skills manifest (`prd-skill-split.md:124-131`) to include `loopdeck-epic-author` in the skills list

## Open Questions

- **Slug derivation.** Should the skill derive the slug from Q1 automatically
  (kebab-case of the outcome), or ask for it explicitly? **Lean:** derive,
  show the user, let them edit. Auto-derivation reduces one question; the
  user confirms anyway in Step 5.
- **How many PRDs to propose?** The skill could propose one PRD per in-scope
  item (fine-grained) or group coupled items (coarser). **Lean:** group
  coupled items per the "one PRD per independent deliverable" rule, and let
  the user split in Step 2. Start coarser; splitting is cheaper than merging.
- **Should the skill draft the `## Design` section or leave it a stub?**
  **Lean:** leave a stub in 0.2.0. The Design section is where
  implementation detail lives, and the human is better-positioned to fill it
  in once they start working. The skill drafting a Design sketch risks
  baking in architectural decisions before the human has made them.
- **Should the skill write ADRs?** **Lean:** no — leave a single placeholder
  ADR (`### ADR-1: <title> — fill in`) so the user knows the shape. ADRs
  capture decisions; the skill hasn't made any yet. The placeholder is a
  formatting aid, not content.
- **Natural-language trigger phrasing.** The frontmatter `description` must
  trigger on "draft a new epic", "structure this feature", "elaborate this
  goal into a spec", but NOT on "help me plan the roadmap" (standing intent)
  or "what should I build next" (open-ended). **Lean:** enumerate the
  on-demand trigger phrases explicitly in the `description` and rely on the
  user's `/loopdeck:epic-author` invocation as the primary path. Revisit
  trigger reliability once the 0.3.0 app-invoked front-end lands (where
  triggering is structured, not natural-language).
- **Updating `## PRD Index` when adding a PRD to an existing epic.** The
  skill adds a row to the table — but what if the user has hand-edited the
  table format? **Lean:** match the existing table's column shape exactly;
  if the table is unparseable, append the row at the end with a comment
  rather than reformatting. Never rewrite a human-authored table.
