---
name: loopdeck:loop-runner
description: Execute a single loop faithfully. Use when there is an active loop in .loopdeck/loops.md ## Current and the user wants it implemented — read the Goal, do the work, record the result. Mechanics only: it executes one loop as written; it never re-plans, decomposes, or spawns planning agents. If the loop carries Epic/PRD back-references, it loads the origin PRD as context for why, not as a mandate to reorganize.
allowed-tools: [Read, Edit, Write, Glob, Grep, Bash]
---

# Loop Runner — Faithful Execution of One Loop

This skill executes **exactly one loop**: the entry in `.loopdeck/loops.md` under
`## Current`. It is the runtime counterpart to the spec layer — the human (or
the `loopdeck:epic-author` skill) authored the plan; this skill carries it out
without re-litigating it. The agent stays **unaware of epics**: a loop's
`**Epic**`/`**PRD**` back-references are read as *context*, never as a mandate.

## The Loop Lifecycle

### 1. Read the current loop

Read `.loopdeck/loops.md` and find the `## Current` block. The `**Goal**` field
is the **only** field that drives your work:

```
## Current
- **Started**: YYYY-MM-DD
- **Goal**: <what to implement — verb-led, single, checkable>
- **Status**: in_progress
- **Epic**: <slug>      ← read-only context (optional)
- **PRD**: <prd-name>   ← read-only context (optional)
```

- If `## Current` is empty or missing, stop and tell the user there is no active
  loop. Do not invent one.
- The `**Goal**` is one sentence. If it is genuinely ambiguous in a way that
  blocks progress, **ask** the user — do not guess and do not re-decompose.

### 2. Read-context rule (why the loop exists, not what to do)

If the `## Current` block carries `**Epic**` and `**PRD**`, read the origin PRD
at `docs/epics/<Epic-slug>/<PRD>.md` as **context for why this loop exists** —
its `## Goals`, the phase it sits in, the surrounding loops. Use it to make
better local decisions (naming, scope, fit with neighbors).

**Treat the PRD as read-only context, not a mandate.** Do **not**:
- re-decompose the plan into different phases,
- edit, reorder, or add to the PRD's checklist,
- spawn agents to re-plan what the human already authored,
- expand scope beyond the single `**Goal**`.

The plan is settled. Your job is to execute this one loop well.

### 3. Implement the Goal

Do the work the `**Goal**` describes. This is the substantive part — write the
code, the test, the doc, whatever the loop is. Keep it scoped to one loop: if
the work sprawls into multiple sessions or a sub-epic, that is a signal the loop
was too big — finish a coherent slice, record progress, and let the human split
the remainder into new loops.

### 4. On completion — record the result

When the loop is done (or abandoned), update `.loopdeck/` by following the
`loopdeck:memory` conventions (do not freehand the formats):

1. Move the `## Current` entry into `## History` as a `### YYYY-MM-DD — Title`
   entry with `- **Status**: completed` (or `abandoned`) and `- **Completed**:
   YYYY-MM-DD`.
2. Clear `## Current` (set it to `_No active loop._` or start the next loop if
   one is queued in `## Next Steps`).
3. **If the completed loop carried `**Epic**`/`**PRD**` back-references**, check
   the matching `- [ ]` box in the origin PRD at
   `docs/epics/<Epic>/<PRD>.md` (find the item matching the loop's `**Goal**`,
   change `- [ ]` → `- [x]`; skip silently if not found).
4. Append any architectural decision made during the loop to `decisions.md`.

Delegate every `.loopdeck/` write above to the formats in `loopdeck:memory`.

## Important Rules

- **One loop.** Execute the single `## Current` Goal. Do not chain into the next
  loop automatically; the human promotes the next one.
- **Goal is law; Epic/PRD is context.** Never edit the back-reference bullets;
  never treat the PRD as an instruction to reorganize.
- **Mechanics, not strategy.** No re-planning, no decomposition, no spawning of
  planning agents. If the plan looks wrong, surface it to the user — do not
  rewrite it.
- **Memory writes go through `loopdeck:memory`.** Consistent formats are what
  make LoopDeck's UI and future sessions able to read the result.
- **Truthful recording.** If a loop is abandoned or only partially done, say so
  in `## History` — do not mark incomplete work as `completed`.
