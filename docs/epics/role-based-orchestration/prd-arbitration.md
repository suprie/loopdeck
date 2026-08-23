---
prd: prd-arbitration
epic: role-based-orchestration
milestone: "0.6.0"
status: proposed
description: >
  A decision-maker layer on top of the handoff pipeline: arbitration runs
  consume competing artifacts or branches and emit a structured verdict
  with rationale, review gates let one role approve, revise, or reject
  another role's artifact before downstream phases proceed, and escalation
  rules decide agent-decides versus parked-for-human — all recorded in the
  morning report.
---

## Overview

Two mechanisms: **decision-maker runs** (a role-chartered agent consumes
competing outputs — e.g. two parallel branches from a diamond in the plan —
and emits a structured verdict with rationale) and **review gates** (a
phase kind where one role reviews another role's artifact and returns
approve / revise / reject before downstream phases start). Escalation
rules decide which outcomes an agent may settle and which park for the
human; agent-to-agent permission grants remain out of scope (epic non-goal).

## Problem Statement

Parallel role work produces competition (two branches, conflicting
analyses) and quality variance (a dev artifact a QA role should check
before it propagates). Today there is no mechanism for either: multi-agent
sub-runs never interact, and approvals are only ever human↔agent. Without
arbitration, "decision maker" and "QA" roles have nothing to decide or
gate.

## Goals

| Priority | Goal |
|---|---|
| P0 | Arbitration run type: decision-maker consumes competing artifacts/branches, emits a structured verdict + rationale |
| P0 | Review-gate phase kind: one role approves, revises, or rejects another role's artifact; downstream phases respect the outcome |
| P1 | Escalation rules: agent-decides versus park-for-human, with thresholds (destructive, budget, ambiguity) |
| P1 | Adjudication records in the morning report: decider, options considered, rationale, escalations |

## Non-Goals

- Agent-to-agent permission grants — an agent never widens another agent's rights (epic non-goal)
- Voting across N agents (single decision-maker per gate for now)
- Re-opening completed loops automatically — a rejected artifact routes back as a revise instruction, not a stealth re-run
- Auto-merge of the winning branch into main (draft-PR-only autonomy unchanged)

## Design

Stub — points to resolve while implementing:

- Whether a decision-maker run needs a worktree at all (it reads artifacts
  and branches; it may not need a checkout)
- Where escalation thresholds live: run plan, role charter, or global
  config — likely run plan with charter-level defaults
- Verdict shape: reuse the existing PASS/WARN/BLOCK verdict mining
  (`run_executor.rs` extract_verdict) as the emission format so the
  morning report needs no new parsing

## Phases

### Phase 1 — Decision-maker runs

- [ ] `arbitration/decision-run-model` Add an arbitration run type where a decision-maker agent consumes competing artifacts or branches and emits a structured verdict with rationale

### Phase 2 — Review gates + escalation

- [ ] `arbitration/review-gate` Add a review-gate phase kind where one role approves, revises, or rejects another role's artifact before downstream phases proceed
- [ ] `arbitration/escalation-rules` Encode escalation rules that decide when a gate outcome is agent-decided versus parked for the human

### Phase 3 — Report

- [ ] `arbitration/report-records` Surface adjudications (decider, options considered, rationale, escalations) in the morning report

### Phase 4 — Tests/verification

- [ ] `arbitration/tests` Add Rust tests for gate outcomes and escalation paths; run an end-to-end two-branch competition resolved by a decision-maker run

## Open Questions

- Can arbitration re-open a completed phase, or only gate downstream ones?
- What happens when the decision-maker itself returns an ambiguous verdict — park, retry, or escalate by default?
- Should a revise outcome auto-queue the author role with the reviewer's notes, or park for the human to re-plan?
