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
| `.loopdeck/decisions.md` | Lightweight ADRs (architectural decision records) | After any significant design/architecture decision |
| `.loopdeck/loops.md` | Current loop status, next steps, history | At the end of every session / loop |

## decisions.md Format

Write decisions as level-2 headings with date and title, followed by key-value
bullets and optional body text:

```markdown
## YYYY-MM-DD — Title of the decision
- **Status**: proposed | accepted | superseded
- **Context**: Why this decision was needed.
- **Consequences**: What follows from this decision.

Additional body text explaining the decision in more detail.
```

**Rules:**
- Use `## YYYY-MM-DD — Title` format (em dash preferred, hyphen accepted)
- `Status` must be one of: `proposed`, `accepted`, `superseded`
- **Append** new decisions — never delete or reorder old ones
- `Context` explains the situation; `Consequences` captures what changed

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

### On completion: check the origin PRD box

**When moving a loop to History, if its `## Current` block carried `**Epic**` and
`**PRD**` back-references, check the matching `- [ ]` box in the origin PRD
file.** The PRD lives at `docs/epics/<Epic-slug>/<PRD>.md`, under a `## Phases`
→ `### Phase N` checklist. Find the item whose text matches the loop's `**Goal**`
and change `- [ ]` to `- [x]`. This keeps the spec layer in sync with what's
actually been built. If the item isn't found (title drifted, or the file was
removed), skip silently — the human can check it manually in the UI.

## When to write

- **After any architectural decision** → append to `decisions.md`
- **At the end of each session/loop** → update `loops.md` Current status + Next
  Steps; move completed loops to History (and check the origin PRD box)
- **When the active-loop summary changes** → update `current-loop.md`

Keep entries concise and factual. These files are read by humans, AI agents, and
LoopDeck's parser — clarity beats volume. Large files cost tokens every time
they are re-read, so prefer appending tight entries and archiving old History to
`loops-archive.md` when `loops.md` grows past ~1500 lines.
