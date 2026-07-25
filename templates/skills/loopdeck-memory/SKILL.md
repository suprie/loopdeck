---
name: loopdeck:memory
description: The .loopdeck/ project-memory write conventions. Use when you (or the user) need to record an architectural decision, update the current loop, move a loop to history, or write any project memory — so decisions.md / loops.md / current-loop.md stay consistently formatted and LoopDeck's UI can parse them. Mechanics only — formats and rules, never planning.
allowed-tools: [Read, Edit, Write, Glob, Grep]
---

# LoopDeck Memory — `.loopdeck/` Write Conventions

This project uses `.loopdeck/` for persistent project memory. These are the
formats and rules for writing to those files so LoopDeck's UI displays them
correctly and future sessions can read them. **Mechanics only** — how to
format and place memory; this skill never decides *what* to build. Both the
orchestrator (`loopdeck:orchestrator`) and the single-loop runner
(`loopdeck:loop-runner`) delegate here for every `.loopdeck/` write.

## Files

| File | Purpose | When to Write |
|------|---------|---------------|
| `.loopdeck/current-loop.md` | Active loop snapshot (one-line dashboard label) | When the active loop's high-level summary changes |
| `.loopdeck/decisions.md` | **Index** of decisions — one short entry each, most recent ~15 live | After any significant design/architecture decision |
| `.loopdeck/decisions-archive.md` | Overflow for `decisions.md` beyond the live window | When `decisions.md` exceeds ~15 live entries |
| `.loopdeck/loops.md` | Current loop status, next steps, history | At the end of every session / loop |
| `.loopdeck/loops-archive.md` | Overflow for `loops.md ## History` beyond the live window | When `## History` exceeds ~5 live entries |
| `docs/decisions/<slug>.md` or `docs/epics/<epic>/adr-<n>.md` | Long-form rationale for a decision — alternatives considered, judgment calls, verification detail | When a decision's Context or Consequences can't fit in 1-2 sentences each |

**decisions.md and loops.md are an index, not a diary.** They exist to be
cheaply re-read by future sessions — every line in them is a token cost paid
on every future read. Anything long-form (the "why we considered X and
rejected it," "here's everything I verified," multi-paragraph postmortems)
belongs in a linked file, a PR description, or a commit message — never
inline in these two files. This is not a style preference: an earlier
incident burned ~30M tokens/hour because these files grew unbounded and were
re-read every turn (see the 2026-07-19 decision in `decisions.md`). The caps
below are the fix; they are enforced by you, the writer, not by a parser.

## decisions.md Format

Write decisions as level-2 headings with date and title, followed by key-value
bullets — **no body text beyond the three bullets**:

```markdown
## YYYY-MM-DD — Title of the decision
- **Status**: proposed | accepted | superseded
- **Context**: Why this decision was needed, in one sentence.
- **Consequences**: What changed, in one sentence.
```

If the decision needs more than that to be understood — alternatives you
weighed, judgment calls, what you verified, a multi-step rationale — **stop
before you write it into `decisions.md`.** Instead:

1. Write the long-form version to `docs/decisions/<slug>.md` (or, if the
   decision belongs to an active epic, `docs/epics/<epic-slug>/adr-<n>.md`
   alongside that epic's existing ADRs).
2. Add the terse 3-bullet entry to `decisions.md` as normal, with a fourth
   bullet: `- **Detail**: docs/decisions/<slug>.md`.

**Rules:**
- Use `## YYYY-MM-DD — Title` format (em dash preferred, hyphen accepted)
- `Status` must be one of: `proposed`, `accepted`, `superseded`
- **Append** new decisions — never delete or reorder old ones
- `Context` explains the situation; `Consequences` captures what changed
- **Hard cap: 3 bullets + 1 optional `**Detail**` link, nothing else.** No
  paragraphs, no inline verification notes, no "also confirmed X, Y, Z" lists.
  If you're tempted to add a fourth sentence to `Context` or `Consequences`,
  that's the signal to write a `Detail` doc instead.
- **When `decisions.md` holds more than ~15 entries**, move the oldest ones
  to `decisions-archive.md` (create it with a `# Decisions Archive` heading
  if absent), leaving a pointer at the top of `decisions.md`:
  `_Older decisions archived to [decisions-archive.md](./decisions-archive.md)._`
  Do this as part of the same write that would push the count over 15 —
  don't wait for someone to notice the file is huge.

## current-loop.md Format

A single line of plain text — the high-level summary of the active loop.
Displayed on the LoopDeck dashboard project card.

```markdown
UI restyling — Tailwind CSS v4, OKLCH dark palette, sidebar layout
```

**Rules:**
- **Max 100 characters** — this is a dashboard card label, not a description
- **Single line** — no markdown bullets, no headings, no newlines
- **High-level summary only** — what is being worked on right now, one sentence
- Keep details (start date, status, next steps) in `loops.md`

## Structured execution state (`execution.yaml`) — check this first

Before editing `loops.md`, check which runtime mode the project is in:

```bash
test -f .loopdeck/execution.yaml && echo structured || echo legacy
```

- **Structured mode** (`.loopdeck/execution.yaml` exists): it is the
  **authoritative** current loop / queue / history. **Never hand-edit it** —
  free-form edits bypass validation and the optimistic-concurrency guard.
  Transition loops exclusively through the validated `loopdeck state` CLI
  (same Rust path the UI uses):

  | Action | Command |
  |--------|---------|
  | Read state | `loopdeck state show` |
  | Start a PRD loop | `loopdeck state promote <loop-id>` |
  | Complete the current loop | `loopdeck state complete [--commit <sha>]` |
  | Abandon the current loop | `loopdeck state abandon --reason "<text>"` |

  `--path` defaults to the current directory (run from the project root). The
  `<loop-id>` is the stable ID from the PRD checklist (e.g.
  `structured-state/parse-loop-id`). `decisions.md` is still hand-edited — it
  is not part of execution state. Treat any `loops.md` as a legacy artifact.
- **Legacy mode** (only `loops.md` exists): use the `loops.md` format below.
  Migration to `execution.yaml` is a one-time, human-confirmed action (Phase 4) —
  the project's Loops tab offers a preview + "Migrate" button, or run
  `loopdeck state migrate` (preview) / `loopdeck state migrate --yes` (apply).
  It matches records to PRD loop IDs by exact title (never guessed), renames the
  original `loops.md` → `loops.legacy.md`, and writes `execution.yaml`. Do not
  hand-edit `loops.md` to "migrate" it — always use the confirmed command/UI.

When a loop completes: in **structured mode** run `loopdeck state complete`; in
**legacy mode** move it to History as described below. In both modes, if the
loop was promoted from an epic/PRD, check the origin PRD box (next section).

## loops.md Format

Write loops as level-2 sections for Current/Next Steps/History, with level-3
entries for historical loops:

```markdown
## Current
- **Started**: YYYY-MM-DD
- **Goal**: What this loop aims to accomplish
- **Status**: in_progress

## Next Steps
- [ ] Task one
- [ ] Task two

## History

### YYYY-MM-DD — Completed loop title
- **Status**: completed
- **Completed**: YYYY-MM-DD
- **Summary**: One sentence — what shipped.
```

### Epic / PRD back-references (when a loop was promoted from the spec layer)

When a loop was promoted from an epic/PRD (via the LoopDeck UI), the `## Current`
block carries back-reference bullets that trace it back to the spec layer:

```markdown
## Current
- **Started**: YYYY-MM-DD
- **Goal**: Define Epic and Prd structs in epic.rs
- **Status**: in_progress
- **Epic**: support-project-management
- **PRD**: prd-spec-layer
```

Treat `**Epic**` and `**PRD**` as **read-only context** — they tell you *why* the
loop exists. Never edit, remove, or reorder them. The `**Goal**` is the only
field that drives your work.

**Rules:**
- `## Current` contains the active loop (or `_No active loop._` if none)
- `## Next Steps` is a checklist of `- [ ]` items for the current loop
- `## History` contains completed/abandoned loops as `### YYYY-MM-DD — Title` entries
- At the end of every session, update the Current loop status and Next Steps
- When a loop completes, move it to History and start a new Current loop
- **Hard cap: `**Status**`, `**Completed**`, `**Summary**` (one sentence), nothing
  else.** Do not add "what I verified," "judgment calls made," "follow-ups,"
  or `cargo`/`tsc` gate results as bullets or prose under a History entry —
  that detail belongs in the PR description (`loopdeck:open-pr` builds one
  from the same context) or the commit message, not here. If a `**Summary**`
  is growing past one sentence, that's the signal to trim it and let the PR
  carry the rest.
- **When `## History` holds more than ~5 entries**, move the older ones to
  `loops-archive.md` (append under its existing heading, oldest-first as
  already established there), leaving the 5 most recent live in `loops.md`.
  Do this in the same write that would push the count over 5.

### On completion: check the origin PRD box

**When moving a loop to History, if its `## Current` block carried `**Epic**` and
`**PRD**` back-references, check the matching `- [ ]` box in the origin PRD
file.** The PRD lives at `docs/epics/<Epic-slug>/<PRD>.md`, under a `## Phases`
→ `### Phase N` checklist. Find the item whose text matches the loop's `**Goal**`
and change `- [ ]` to `- [x]`. This keeps the spec layer in sync with what's
actually been built. If the item isn't found (title drifted, or the file was
removed), skip silently — the human can check it manually in the UI.

## When to write

- **After any architectural decision** → append the 3-bullet index entry to
  `decisions.md`; if it needs more explanation than that, write the long form
  to `docs/decisions/<slug>.md` first and link it back
- **At the end of each session/loop** → update `loops.md` Current status + Next
  Steps; move completed loops to History as a `**Summary**` one-liner (and
  check the origin PRD box)
- **When the active-loop summary changes** → update `current-loop.md`
- **Whenever a write would push `decisions.md` past ~15 live entries or
  `loops.md ## History` past ~5** → archive the overflow in the same write

These files are read by humans, AI agents, and LoopDeck's parser on every
session start — every line is a token cost paid repeatedly, not once. Treat
them as an index you'd want to skim in ten seconds, not a log of everything
that happened. If you find yourself writing more than 3-4 lines for one
entry, that's not thoroughness — it's the file turning into a diary. Put the
detail somewhere it's read once (an ADR file, a PR body, a commit message)
instead of somewhere it's re-read every turn.
