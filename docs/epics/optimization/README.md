---
title: Optimization
slug: optimization
milestone: "0.4.0"
status: proposed
started: 2026-07-28
owner: Suprie
description: >
  Pay down the debt a fresh honest review surfaced: docs/PRD.md and
  CLAUDE.md no longer describe the code that ships, AppState's session
  tracking has sprawled into five parallel maps, the frontend has zero
  automated tests, .loopdeck/ memory files are re-bloating past their last
  documented incident threshold, and recent loop history shows the project's
  own process consuming more effort than its features. No new product
  surface — this epic only makes the existing one accurate, smaller, tested,
  and cheaper to run agents against.
---

# Epic — Optimization

## Motivation

A review across docs/PRD.md, .loopdeck/decisions.md, .loopdeck/loops.md,
src-tauri/src/lib.rs, src-tauri/src/commands/, and src/ turned up five
concrete problems, none of them new features:

1. **Docs drift.** docs/PRD.md is frozen at "V1 done 2026-06-22" and states
   non-goals ("no agent execution", "no decision tracking") that the shipped
   code violates outright — `claude_session.rs` (2605 lines), `codex_session.rs`,
   `agents.rs`, `epic.rs`, `execution.rs`, and 20+ agent IPC commands
   (`lib.rs:115-183`) all exist and ship. CLAUDE.md still describes
   `commands.rs` as one 30K-token file with "12 commands"; it's actually a
   `commands/` directory with roughly 45 handlers. Anyone — human or agent —
   who trusts these docs to onboard gets a false map.
2. **State sprawl.** `AppState` (`lib.rs:107-113`) carries five parallel
   `Mutex<HashMap>` pending-slot maps for agent/session tracking, and
   `commands/agent.rs` has grown to 1731 lines. This is the most
   concurrency-sensitive surface in the app and it's the least consolidated.
3. **Zero frontend tests.** 459 Rust tests exist and are solid. Streaming
   channels, migration cards, and permission-approval flows in `src/` have
   no automated coverage at all — the weakest correctness surface in the app.
4. **Memory bloat, again.** `loops.md` (~37K tokens) and `decisions.md`
   (~74K tokens) are recreating the exact failure mode `decisions.md`
   already documents from 2026-07-19 (a 30M-token/hour incident from
   re-reading bloated memory files). The archive-at-90KB mechanism is being
   outgrown a second time.
5. **Process eating product.** Recent loop history includes checkbox
   reconciliation loops auditing prior reconciliation loops, PRD amendments
   about amendments, and a near-miss where an agent almost deleted the
   orchestrator by misreading its own memory. This project is maintained by
   one person on top of two vendor CLIs (`claude`, `codex app-server`) that
   can shift protocol without notice — ceremony overhead is a real cost, not
   a free good.

None of these require new UI or new agent capability. They require the specs
to match reality, the state to consolidate, a test floor to exist, the
memory files to shrink and stay shrunk, and a standing rule that stops the
next reconciliation loop from spawning another one.

## Scope

In scope:

- Audit and rewrite `docs/PRD.md` and `CLAUDE.md` so both describe the code
  that actually ships (feature list, non-goals, file/command counts).
- Consolidate `AppState`'s five `Mutex<HashMap>` session-tracking maps
  (`lib.rs:107-113`) into a single `SessionState` struct; right-size
  `commands/agent.rs` if it's still oversized afterward.
- Stand up a frontend test toolchain and cover the three flagged gaps:
  streaming channels, migration cards, permission-approval flow.
- Tighten `.loopdeck/` memory hygiene: a token budget and entry-length
  convention for `loops.md`/`decisions.md`, a lower archive trigger, and a
  one-time compaction of current content.
- Write down a standing guardrail against process-about-process loops and
  wire it somewhere it's actually checked (loop-runner skill and/or the Stop
  hook), not just stated in a memory file no one re-reads.

Out of scope:

- Any new product feature or new agent capability.
- Changing the `claude`/`codex app-server` integration protocol itself.
- 100% frontend coverage — this closes the three named gaps, not a general
  coverage mandate.
- Rewriting the `.loopdeck/` memory *format* — `loopdeck-memory` skill
  mechanics stay; this tightens the budget and convention within them.
- Re-litigating milestone scope for 0.3.0/0.4.0 feature epics — this epic
  runs alongside them, not instead of them.

## PRD Index

| PRD | Covers |
|-----|--------|
| [prd-docs-accuracy.md](./prd-docs-accuracy.md) | Audit + rewrite of `docs/PRD.md` and `CLAUDE.md` against actual code |
| [prd-session-state-consolidation.md](./prd-session-state-consolidation.md) | `AppState` map consolidation into `SessionState`, `commands/agent.rs` right-sizing |
| [prd-frontend-test-coverage.md](./prd-frontend-test-coverage.md) | Frontend test toolchain + coverage for streaming/migration/permission flows |
| [prd-memory-hygiene.md](./prd-memory-hygiene.md) | Token budget + convention + compaction for `loops.md`/`decisions.md` |
| [prd-process-discipline.md](./prd-process-discipline.md) | Standing guardrail against reconciliation-about-reconciliation loops |

## Architecture Decisions

### ADR-1: <title> — fill in

## Success Criteria

- `docs/PRD.md` and `CLAUDE.md` state no non-goal that the shipped code
  violates, and describe the real `commands/` structure and command count.
- `AppState` has one `SessionState` field where it had five `Mutex<HashMap>`
  fields; `cargo test` passes unchanged.
- A frontend test command exists (`npm test` or equivalent) and covers at
  least one streaming-channel case, one migration-card case, and one
  permission-approval case; CI or a documented local command runs it.
- `loops.md` and `decisions.md` each have a stated token budget, are under
  it after compaction, and the archive trigger fires below the old 90KB
  threshold.
- A written guardrail rule exists (not just a decisions.md entry) that a
  loop-runner or Stop-hook check can actually enforce, and it's exercised
  against at least one of the recent reconciliation-loop cases from
  `loops.md`/`decisions.md` history as a dry run.

## Risks

| Risk | Mitigation |
|------|-----------|
| Consolidating `AppState`'s five session maps introduces a race or deadlock in the most concurrency-sensitive part of the app | `prd-session-state-consolidation.md` opens with a spike phase that maps every current call site and lock-ordering assumption before any code moves; existing Rust test suite must stay green throughout. |
| Docs rewrite is itself treated as a one-time event and drifts again within a milestone | `prd-docs-accuracy.md` includes a "keep it honest" check as part of the guardrail work in `prd-process-discipline.md`, not a standalone one-off. |
| Memory compaction loses context that later loops actually needed | Compact by archiving, never deleting; `prd-memory-hygiene.md` verifies the archive is readable and indexed, not just shorter. |
| This epic itself becomes another process-about-process exercise | Every PRD below ends in a phase that produces a concrete artifact (a passing test, a rewritten doc, a smaller file) — no phase whose only output is another audit of this epic. |
