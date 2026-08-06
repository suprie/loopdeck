---
prd: prd-docs-accuracy
epic: optimization
milestone: "0.4.0"
status: completed
description: >
  Audit docs/PRD.md and CLAUDE.md against the code that actually ships, then
  rewrite both so neither states a non-goal the code violates or a file
  structure that no longer exists. No code changes — documentation only.
order: 10
---

# PRD — Docs Accuracy

## Overview

`docs/PRD.md` is frozen at "V1 done 2026-06-22" and lists non-goals ("no
agent execution", "no decision tracking") that the shipped code violates:
`claude_session.rs`, `codex_session.rs`, `agents.rs`, `epic.rs`,
`execution.rs`, and 20+ agent IPC commands (`lib.rs:115-183`) all exist and
ship. `CLAUDE.md` still describes `commands.rs` as one file with "12
commands"; it's actually a `commands/` directory with roughly 45 handlers.
This PRD closes the gap between what the docs claim and what `git log` and
the source tree actually show.

## Problem Statement

Two documents are load-bearing for onboarding — human and AI agent alike —
and both are wrong in ways that actively mislead:

- `docs/PRD.md`'s non-goals read as current constraints, not historical
  scope. A reader (or an agent deciding whether a feature is in-bounds) has
  no way to tell "V1 didn't have this" from "this is still forbidden."
- `CLAUDE.md`'s "Project Structure" section names `commands.rs` as a single
  file; the actual layout is a `commands/` directory. The file-size guidance
  in "Context Discipline" references file names that may no longer match
  the tree.

Neither document has a mechanism to catch this drift as it happens — that's
`prd-process-discipline.md`'s job. This PRD is the one-time catch-up.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Produce a diff list: every claim in `docs/PRD.md` and `CLAUDE.md` that contradicts the current source tree. |
| P0 | Rewrite `docs/PRD.md` so its status/non-goals section reflects shipped reality, with superseded claims marked historical rather than deleted. |
| P0 | Rewrite `CLAUDE.md`'s "Project Structure" and "Context Discipline" sections to match the actual `src-tauri/src/` and `commands/` tree. |
| P1 | Cross-link `docs/PRD.md` to the epics that shipped the excluded-in-V1 features (`agent-full-access`, `overnight-orchestration`, `support-project-management`), so the historical record is traceable. |

## Non-Goals

- Rewriting `docs/PRD.md`'s product vision or adding new requirements —
  this PRD corrects factual drift, it does not re-scope the product.
- Auditing every epic/PRD file under `docs/epics/` — scope is `docs/PRD.md`
  and `CLAUDE.md` only.
- Automated doc-drift detection — that's the guardrail work in
  `prd-process-discipline.md`; this PRD is the manual one-time fix.

## Design

_Stub — fill in once the audit (Phase 1) produces the actual diff list. The
rewrite approach (mark-historical vs. delete-and-replace) should be decided
against real examples, not speculatively._

## Phases

### Phase 1 — Audit

- [x] `optimization/diff-docs-prd-md-s-stated-non-goals-and-v1-scope-against-the` Diff `docs/PRD.md`'s stated non-goals and "V1" scope against the
      current `src-tauri/src/` tree; list every contradiction with
      file:line evidence.
- [x] `optimization/diff-claude-md-s-project-structure-and-file-size-callouts` Diff `CLAUDE.md`'s "Project Structure" and file-size callouts
      (`commands.rs`, `claude_session.rs`, `conversation.rs`, `agents.rs`,
      `epic.rs`, `config.rs`) against the actual current file sizes and
      layout; list every contradiction.

### Phase 2 — Rewrite

- [x] `optimization/rewrite-docs-prd-md-s-status-non-goals-section-to-reflect-shipped` Rewrite `docs/PRD.md`'s status/non-goals section to reflect shipped
      reality, marking superseded items as historical with a date and a
      pointer to the epic that shipped them.
- [x] `optimization/rewrite-claude-md-s-project-structure-section-to-match-the-actual` Rewrite `CLAUDE.md`'s "Project Structure" section to match the actual
      `commands/` directory and current large-file list.

### Phase 3 — Verification

- [x] `optimization/re-run-the-phase-1-diff-against-the-rewritten-docs-confirm-zero` Re-run the Phase 1 diff against the rewritten docs; confirm zero
      remaining contradictions.
- [x] `optimization/have-loopdeck-prd-verifier-or-a-manual-pass-confirm-the-rewritten` Have `loopdeck-prd-verifier` (or a manual pass) confirm the rewritten
      `docs/PRD.md` accurately states current scope before this PRD is
      marked accepted.

## Open Questions

- Should historical non-goals move to an "Amendments" section at the top of
  `docs/PRD.md` (matching the pattern already used in
  `prd-full-access-tier.md`), or a separate changelog file? Resolve during
  Phase 1 based on what the audit actually turns up.
