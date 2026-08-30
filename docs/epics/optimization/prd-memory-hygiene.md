---
prd: prd-memory-hygiene
epic: optimization
milestone: "0.4.0"
status: accepted
description: >
  Define a token budget and entry-length convention for .loopdeck/loops.md
  and decisions.md, lower the archive trigger below the 90KB threshold that
  already proved insufficient once, and compact current content to fit —
  without deleting anything, only archiving it.
order: 40
---

# PRD — Memory Hygiene

## Overview

`decisions.md` documents a 2026-07-19 incident where bloated memory files
caused a 30M-token/hour re-reading cost. The fix at the time was an
archive-at-90KB mechanism. `loops.md` (~37K tokens) and `decisions.md`
(~74K tokens) are now recreating the same problem the fix was meant to
prevent — the threshold and the entry-length habits that caused it were
never actually changed, just the archive point was added. This PRD lowers
the trigger, sets a convention that keeps entries short in the first place,
and does a one-time compaction.

## Problem Statement

- The 90KB archive trigger only caps total file size; it does nothing to
  stop individual loop/decision entries from being 500-word essays, which
  is what actually drives the token cost per read.
- There's no stated per-entry length convention, so each new entry is as
  long as whoever writes it feels like making it.
- Compaction, if done carelessly, risks losing context a later loop
  actually needs — this must archive, not delete.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Define a token budget for the *active* (non-archived) portion of `loops.md` and `decisions.md`, well under the current ~37K/~74K sizes. |
| P0 | Define an entry-length convention (e.g. a target line/word count per loop and per decision) and add it to the `loopdeck-memory` skill's format rules. |
| P0 | Lower the archive trigger below the current 90KB threshold to match the new budget. |
| P1 | Compact current `loops.md`/`decisions.md` content to fit the new budget by archiving older entries, not deleting them. |
| P2 | Verify the archive remains readable and indexed (not just moved out of sight) after compaction. |

## Non-Goals

- Changing the `.loopdeck/` file *format* (frontmatter, section headers) —
  only the size budget and entry-length convention change.
- Rewriting historical decisions for accuracy — that's a content concern,
  not a hygiene concern; leave historical entries' content as-is, just
  archive them if they're old.
- Automating compaction as an ongoing background job — this PRD sets the
  convention and does the one-time catch-up; ongoing enforcement is
  `prd-process-discipline.md`'s concern if it needs a check at all.

## Design

_Numbers picked against Phase 1's measurement (`.loopdeck/memory-budget-report.md`),
not guessed: pre-compaction `loops.md` ~40.0K tokens / `decisions.md` ~8.2K
tokens (chars/4); median entry 252-297 tokens; worst single entry 4,322
tokens — entry length, not count, dominates cost._

- **Token estimate method**: chars/4 (`wc -c`, offline) — pinned by the
  run's pre-answered clarification (no tokenizer, must work offline).
- **Active-file budget**: 3,000 tokens (~12KB) per file, whole file —
  matches the proven post-2026-07-19-incident healthy size (~11KB).
- **Archive trigger**: the write that would push an active file past
  2,400 tokens (~9.6KB) — ~10x below the retired de-facto 90KB trigger.
- **Entry-length convention**: new decisions entries ≤ 60 words (3 bullets
  + optional `Detail` link); new loops History `**Summary**` ≤ 50 words;
  live-entry ceiling ~300 words / ~400 tokens (over → archive or split to a
  `Detail` doc, never rewrite in place).
- **Conflict rule**: the token budget supersedes the skill's count windows
  (~15 decisions / ~5 history) wherever they conflict.
- **Enforcement**: document-only (run's pre-answered clarification);
  automated enforcement, if ever wanted, belongs to
  `prd-process-discipline.md`.

## Phases

### Phase 1 — Define the budget and convention

- [x] Measure current `loops.md`/`decisions.md` token counts and the
      distribution of entry lengths (shortest/median/longest) to set a
      realistic target budget.
- [x] Write the token budget, entry-length convention, and new archive
      trigger into the `loopdeck-memory` skill's format rules.

### Phase 2 — Compact

- [x] Archive older/completed `loops.md` entries down to the new budget,
      preserving them in the existing archive location (not deleting).
- [x] Archive older `decisions.md` entries the same way, keeping recent/
      still-relevant decisions in the active file.

### Phase 3 — Verification

- [x] Confirm both active files are under the new budget.
- [x] Confirm the archived content is still readable and findable (an
      index or pointer from the active file, matching whatever pattern
      `loopdeck-memory` already documents for archives).

## Open Questions

- ~~Should the entry-length convention be enforced automatically (a
  Stop-hook check) or just documented and trusted?~~ **Resolved 2026-08-30**
  (run's pre-answered clarification): **document-only** — the convention
  lives in the `loopdeck-memory` skill's format rules and is enforced by the
  writer. If automated enforcement is ever wanted, `prd-process-discipline.md`
  owns it, not this PRD.
