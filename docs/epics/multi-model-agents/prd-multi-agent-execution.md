---
prd: prd-multi-agent-execution
epic: multi-model-agents
milestone: "0.4.0"
status: proposed
description: >
  Let a user assign one or more named agent configs to a loop and run them
  concurrently, each in its own isolated worktree, with output tagged per
  agent in the UI.
---

## Overview

Today a loop resolves one `AgentConfig` and spawns one `HarnessSession`
(`src-tauri/src/commands/agent.rs:1641`). This PRD lets a loop be started
with N named configs (from the prd-agent-config roster) and spawns N
sessions concurrently, reusing the linked-worktree isolation already built
for overnight-orchestration (`src-tauri/src/git.rs:44` `worktree_add`,
`:76` `worktree_remove`) so concurrent agents never share a checkout.

## Problem Statement

A user cannot compare how different models/backends handle the same task
without manually reconfiguring and re-running a loop serially. There is also
no runtime concept of "run these N agents on this loop" — assignment,
concurrent spawn, and per-agent output attribution all need to be built.

## Goals

| Priority | Goal |
|---|---|
| P0 | A loop can be assigned N named agent configs before it runs |
| P0 | N `HarnessSession`s spawn concurrently, each in an isolated worktree |
| P0 | Loop view output/transcripts are tagged by which named config produced them |
| P1 | Per-agent run status (running/done/failed) surfaced independently in the UI |

## Non-Goals

- Result comparison/merge UI (epic non-goal).
- Automatic agent selection — the user always assigns configs manually.

## Design

<!-- Stub — fill in during implementation. Key open question: does each
concurrent agent get its own loop entry in loops.md, or one loop with N
sub-runs? This determines how deep the worktree-per-run reuse goes. -->

## Phases

### Phase 1 — Spike: worktree isolation under N concurrent sessions

- [ ] Prototype spawning 2-3 `HarnessSession`s concurrently, each against a `worktree_add`-created worktree, and confirm no shared-state write races (particularly `.loopdeck/loops.md` on the main tree); write findings back into this PRD's Design section

### Phase 2 — Per-loop agent assignment (backend + IPC)

- [ ] Extend the loop-start path (`agent.rs:1641` and callers) to accept a list of named config references instead of resolving the single global config

### Phase 3 — Concurrent execution engine

- [ ] Spawn one `HarnessSession` per assigned config inside its own `worktree_add`-created worktree, running concurrently; clean up via `worktree_remove` on completion per the spike's findings

### Phase 4 — Per-agent output/transcript tagging (frontend)

- [ ] Tag each session's transcript/output with its source config name in `src/components/`, so the loop view distinguishes concurrent agents' output

### Phase 5 — Tests/verification

- [ ] Rust tests for concurrent spawn/cleanup; verify against prd-agent-config's roster once merged

## Open Questions

- Does each concurrent agent get its own `.loopdeck/loops.md` entry, or one loop with N tagged sub-runs?
- How are conflicting/duplicate results from concurrent agents surfaced to the user, given result comparison UI is explicitly out of scope?
