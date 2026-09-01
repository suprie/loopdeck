---
prd: prd-agent-handoff
epic: role-based-orchestration
milestone: "0.6.0"
status: proposed
description: >
  A file-based handoff store under .loopdeck/: upstream phases emit contract
  artifacts, dependent phases get them injected into their prompts, phases
  with unmet handoff dependencies report Waiting instead of parking, and
  every handoff lands in the run report as an auditable from-role to-role
  ledger.
---

## Overview

Implements ADR-2 (communication is files on disk): a handoff store under
`.loopdeck/`, artifacts written by the run executor when a phase completes,
injection of upstream artifacts into dependent phases' prompts, and the
currently-unused `Waiting` sub-run status (`multi_agent.rs:35`) wired to
mean "blocked on an upstream handoff, not on a human".

## Problem Statement

Phases in a run plan share state only via files in the worktree and one
combined prompt; there is no notion of agent B consuming agent A's output
as a first-class, auditable handoff. Parallel and sequential role work both
need that join before arbitration or specialization mean anything.

## Goals

| Priority | Goal |
|---|---|
| P0 | Handoff store layout + artifact record (author role, phase, type, path) with load/save helpers |
| P0 | Executor emits each finished phase's contract artifact into the store |
| P0 | Downstream injection: dependent phases receive upstream artifacts, bounded in size, contract-cited |
| P0 | Waiting status wired for unmet handoff dependencies (distinct from Parked) |
| P1 | Handoff ledger in the run report: from-role, to-role, artifact path, citation check result |

## Non-Goals

- Any message bus, socket, or non-file channel (ADR-2)
- Parallel execution — this PRD works sequentially; concurrency is prd-parallel-scheduling
- Arbitration or review of artifacts (prd-arbitration)
- Interactive sessions writing to the store (unattended runs only, for now)

## Design

Filled from prd-handoff-spike findings (2026-09-01 run; full evidence in
[handoff-spike-run.md](handoff-spike-run.md), contract in
[handoff-artifact-contract.md](handoff-artifact-contract.md)):

**Go/no-go: GO.** A path-only-prompted consumer session cited 17/17 artifact
headings + items (coverage), contradicted nothing and fabricated nothing
(fidelity), and dropped or merged nothing (completeness — no truncation).
Ignored-input rate: 0. Single-run sample, both sessions same harness;
operator was the run executor spawning both sessions back-to-back per the
queue-time authorization, deviating from the spike PRD's manual-session
wording.

**Contract adopted as-is for injection sizing:** Markdown + YAML frontmatter
at `.loopdeck/handoffs/<topic>.md`; frontmatter = the artifact record
(author role, phase, type); caps (≤ 8 KiB body hard, ≤ 8 sections, ≤ 12
numbered items) bound downstream injection.

**Design changes the spike forces:**

- The citation rule (§5 of the contract) held **without enforcement** — wire
  it into the run report's citation check as-is, and treat the `##
  Handoff citations` block as the parse target.
- Producer-side **heading drift** was observed (consumer's own `type: plan`
  artifact used free-form headings instead of the schema's). Executor-side
  artifact emission (this PRD's Phase 1) must generate the schema headings
  mechanically rather than trusting the agent — only prompt-driven
  *consumer* behavior proved reliable.
- Watch unanchored open questions: the consumer self-resolved both; benign
  with normative anchors (R4/R3), a drift vector without. Producer rule:
  open questions must cite their anchor item or be split out pre-handoff.

- Persist the handoff ledger graph-native from day one — nodes and links
  (plain JSON), not a flat log. This is the seed of the 0.7.0 "loops graph"
  direction (edges promoted into the spec layer, unified graph state,
  invalidation propagation — Parking Lot in `.loopdeck/loops.md`);
  retrofitting edges onto a flat log later means a migration.

## Phases

### Phase 1 — Handoff store

- [ ] `agent-handoff/store-model` Define the .loopdeck handoff store layout and artifact record (author role, phase, type, path) with load/save helpers
- [ ] `agent-handoff/artifact-write` Emit each finished phase's contract artifact into the store from the run executor

### Phase 2 — Downstream injection + waiting

- [ ] `agent-handoff/downstream-injection` Inject upstream artifacts into a dependent phase's prompt with bounded size and contract citation
- [ ] `agent-handoff/waiting-status` Wire the unused Waiting status so phases with unmet handoff dependencies wait instead of parking

### Phase 3 — Report + audit

- [ ] `agent-handoff/report-attribution` Record every handoff (from-role, to-role, artifact path, citation check) in the run report

### Phase 4 — Tests/verification

- [ ] `agent-handoff/tests` Add Rust tests for store round-trips, injection bounds, and Waiting transitions; verify against the spike's contract

## Open Questions

- Store location: `.loopdeck/handoffs/` per project, or inside `.loopdeck/agent-runs/<run-id>/`?
- Retention policy for artifacts across runs?
- Should interactive sessions be able to *read* the blackboard even if they never write?
