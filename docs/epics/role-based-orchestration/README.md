---
title: Role-Based Agent Orchestration
slug: role-based-orchestration
milestone: "0.6.0"
status: proposed
started: 2026-08-17
owner: Suprie
description: >
  A user staffs a queued plan with specialized role agents — business
  analyst, engineering manager, developer, QA, marketing — and LoopDeck
  routes each phase to the right role, hands work between agents over
  auditable files, and arbitrates competing outputs with a recorded
  decision, instead of fanning one identical prompt out to N model configs.
---

## Motivation

Today "multi-agent" in LoopDeck means an ensemble, not a team: up to 8 named
configs receive the identical prompt in isolated worktrees, and nobody
delegates, reviews, or decides anything (`multi_agent.rs` sub-runs have no
message passing, no handoff, no lead). An "agent" is a connection profile —
harness, model, URL, auth (`config.rs:23`) — with no persona, no system
prompt (zero uses of `--append-system-prompt` in the codebase), and run-plan
phases carry no agent assignment, so the run queue always executes with the
default config. The spec layer has no concept of a marketing agent, a
business analyst, or a QA agent producing anything other than branches and
draft PRs.

This epic turns that ensemble into a staffed pipeline: roles with charters,
phases routed to the right role, work handed between agents over auditable
files, and a decision-maker that adjudicates competing outputs — with the
app, not a lead agent, doing the orchestration.

## Scope

- Role charters on the agent roster: persona/system prompt, allowed skills,
  output contract — plus injection into every spawn path (interactive,
  run-queue, multi-agent)
- Role-scoped autonomy: per-role permission rules above the existing
  destructive floor
- Per-phase agent assignment in run plans
- File-based handoff: a `.loopdeck/` handoff store with artifact contracts,
  downstream prompt injection, and Waiting (not Parked) for unmet handoffs
- Dependency-aware parallel scheduling: one turn per phase, concurrent
  phases where `depends_on` allows, restart reconciliation, per-phase budgets
- Arbitration: decision-maker runs, review gates where one role reviews
  another's artifact, escalation rules (agent-decides vs park-for-human)
- Non-code artifact types (doc, content) so marketing and BA roles produce
  reviewed work without worktree/PR requirements

## Non-Goals

- Recurring schedules (cron) and global cross-project token/cost budgets —
  deferred to a later epic
- Lead-agent-as-orchestrator: an agent that plans and spawns sub-agents
  invisibly to the app is explicitly not built here
- Agent-to-agent permission grants — approvals remain human↔agent only
- Runtime skill injection / app-managed CLAUDE.md, skills, and hooks (still
  parked in `.loopdeck/loops.md` Parking Lot)
- Cloud sync and team collaboration (PRD-level exclusion, unchanged)
- Auto-merge or ready-for-review PRs — draft-only autonomy unchanged
  (overnight-orchestration ADR-1)

## PRD Index

| PRD | Description | Status |
|---|---|---|
| [prd-handoff-spike](prd-handoff-spike.md) | Spike the riskiest unknown: two-agent file handoff on a real plan, prompt text only; go/no-go feeds prd-agent-handoff | proposed |
| [prd-role-foundations](prd-role-foundations.md) | Role charters on the roster, charter injection into every spawn path, role-scoped autonomy, per-phase agent assignment (sequential) | proposed |
| [prd-agent-handoff](prd-agent-handoff.md) | The `.loopdeck/` handoff store: artifact contracts, downstream injection, Waiting status, handoff ledger in the run report | proposed |
| [prd-parallel-scheduling](prd-parallel-scheduling.md) | One turn per phase, concurrent phases where `depends_on` allows, gated starts, restart reconciliation, per-phase budgets | proposed |
| [prd-arbitration](prd-arbitration.md) | Decision-maker runs, review gates between roles, escalation rules, adjudication records in the morning report | proposed |
| [prd-non-code-artifacts](prd-non-code-artifacts.md) | Artifact type model beyond code branches; doc/content pipelines with review gates instead of test gates | proposed |

**Delivery order is index order.** The spike gates the handoff design before
scaffolding is built; role-foundations ships the sequential dev-builds /
QA-verifies demo; handoff and parallel-scheduling build on both; arbitration
and non-code-artifacts layer on top of a working handoff pipeline.

## Architecture Decisions

### ADR-1: The app is the orchestrator
- **Status**: drafted (2026-08-17 session) — ratify or edit
- **Context**: Routing, budgets, and arbitration policy could live in a lead
  agent that plans and delegates. The terminal `loopdeck-orchestrator` skill
  already works that way, human-gated.
- **Consequences**: Routing, dependency scheduling, and escalation thresholds
  live in the Rust run executor where they are deterministic and testable;
  agents never spawn each other. The skill remains the attended path.

### ADR-2: Inter-agent communication is files on disk
- **Status**: drafted (2026-08-17 session) — ratify or edit
- **Context**: Sub-runs need to hand work to each other. A message bus would
  be new infrastructure; files fit the local-first identity.
- **Consequences**: A `.loopdeck/` handoff store with artifact contracts is
  the only channel. Handoffs are git-auditable, human-readable, and survive
  restarts. No bus, no sockets.

### ADR-3: Roles extend the existing roster, not a parallel identity system
- **Status**: drafted (2026-08-17 session) — ratify or edit
- **Context**: `NamedAgentConfig` (`config.rs:76`) already has UUIDs, CRUD,
  and UI. A separate "role" entity would duplicate it.
- **Consequences**: A role charter (persona, allowed skills, output contract)
  layers onto a roster entry; an entry without a charter stays a plain
  connection profile.

## Success Criteria

1. A role-chartered agent's session visibly follows its charter — e.g. a
   QA-role session runs verification and emits a PASS/WARN/BLOCK verdict —
   checkable by inspecting the run transcript
2. Run-plan phases execute with their assigned agent(s), not the default
   config, and the run report attributes output per role
3. Phases with disjoint dependencies run concurrently in separate worktrees
   while a dependent phase reports Waiting until its upstream completes —
   verified on a three-phase diamond plan
4. An artifact written by an upstream role appears in the downstream role's
   prompt and is cited in that role's output — checkable in transcripts and
   the run report
5. When two branches compete, a decision-maker run records a choice with
   rationale; gate outcomes exceeding escalation thresholds park for the
   human

## Risks

- **Handoff reliability** (riskiest unknown): downstream agents may ignore,
  truncate, or drift from upstream artifacts. Mitigation: spike first
  (prd-handoff-spike) before any scaffolding; artifact contracts with
  schemas and size caps; citation checks in the run report.
- **Role charters may yield "generic model with a hat"**: charters might not
  produce genuinely specialized behavior. Mitigation: charter template with
  a hard output contract; acceptance is a distinguishable output shape, not
  tone.
- **Scheduler races**: parallel phases interacting with parking, budgets,
  stall policy, and worktree cleanup. Mitigation: reuse the executor's
  existing restart reconciliation, bounded concurrency, property tests over
  diamond graphs and mid-flight crashes.
- **Non-code artifacts strain PR-shaped infra**: sessions keyed by worktree
  and outputs as draft PRs don't fit marketing/BA work. Mitigation: the
  artifact type model lands before the doc/content pipelines; content stays
  git-backed where possible.
