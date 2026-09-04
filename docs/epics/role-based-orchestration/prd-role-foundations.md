---
prd: prd-role-foundations
epic: role-based-orchestration
milestone: "0.6.0"
status: proposed
description: >
  Give agent configs an identity beyond connection settings: a role charter
  (persona, allowed skills, output contract) injected into every spawned
  session, role-scoped permission rules above the destructive floor, and
  per-phase agent assignment in run plans — so a sequential run can already
  be staffed dev-builds / QA-verifies with per-role attribution.
---

## Overview

Today `NamedAgentConfig` (`config.rs:76`) is a connection profile — harness,
model, base URL, auth — with no persona and no system prompt (the codebase
has zero uses of `--append-system-prompt`). And `RunPhase` (`runplan.rs:120`)
has no agent field: the run queue always executes with the default agent
config. This PRD closes both gaps: charters on the roster, injection into
every spawn path, role-scoped autonomy, and per-phase assignment with the
executor honoring it.

## Problem Statement

Everything else in this epic routes work *to a role*, but there is no role
to route to: an agent config cannot say "you are the QA agent", and a run
plan cannot say "this phase runs with the QA agent". Without these two
joins, specialized agents are unreachable from the spec and execution
layers.

## Goals

| Priority | Goal |
|---|---|
| P0 | Role charter fields (persona prompt, allowed skills, output contract) on roster entries, with migration so existing configs stay valid |
| P0 | Charter injection into spawned sessions across interactive, run-queue, and multi-agent paths, harness-neutral (Claude and Codex) |
| P0 | Per-phase agent assignment in `RunPlan` phases, executed by the run queue |
| P1 | Role-scoped permission rules above the destructive floor |
| P1 | Charter editing in the agent settings UI |

## Non-Goals

- Parallel phase execution (prd-parallel-scheduling)
- Any handoff store or inter-agent artifacts (prd-agent-handoff)
- Arbitration, review gates, or decision-maker runs (prd-arbitration)
- Runtime skill injection — charters reference skills by name; the app does not push skill files at runtime

## Design

Stub — points to resolve while implementing:

- Injection mechanism: CLI flag (`--append-system-prompt` for Claude) versus
  environment wiring in `apply_agent_config` (`agents.rs:1074`); must have a
  Codex-equivalent path so the `HarnessAdapter` trait stays honest
- Composition with the existing loop-prompt boilerplate
  (`build_next_loop_prompt`, `commands/agent.rs:994`) — charter and loop
  prompt must not fight for the same instruction space
- Migration: charter fields optional; an entry without a charter remains a
  plain connection profile (ADR-3)

## Phases

### Phase 1 — Role charter data model

- [x] `role-foundations/charter-model` Extend NamedAgentConfig with role charter fields (persona prompt, allowed skills, output contract) and migrate existing configs
- [x] `role-foundations/charter-crud` Add IPC CRUD for charter fields and surface them in the agent settings editor

### Phase 2 — Charter injection into sessions

- [x] `role-foundations/prompt-injection` Inject the role charter into spawned harness sessions across the interactive, run-queue, and multi-agent paths
- [x] `role-foundations/injection-tests` Assert per-path charter injection with fixtures that capture the child process arguments

### Phase 3 — Role-scoped autonomy

- [x] `role-foundations/role-policy` Extend PermissionPolicy with per-role rules above the destructive floor

### Phase 4 — Per-phase agent assignment

- [x] `role-foundations/phase-assignment-model` Add assigned-agent fields to RunPhase and the create_run_plan selection surface
- [x] `role-foundations/phase-assignment-exec` Make the run-queue executor spawn each phase with its assigned agent instead of the default config
- [x] `role-foundations/role-demo` Run an end-to-end two-phase plan where a dev-role agent builds and a QA-role agent verifies, with per-role attribution in the run report

### Phase 5 — Tests/verification

- [ ] `role-foundations/tests` Add Rust tests for charter migration, injection routing, and assignment; pass prd-verifier against this PRD

## Open Questions

- Charter stored globally on the roster, per-project override, or both?
- Is the output contract validated post-run (parsed and checked) or advisory prose?
- Does a charter's skill allowlist *restrict* tools, or only *suggest* skills in the prompt?
