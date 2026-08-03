---
prd: prd-multi-agent-execution
epic: multi-model-agents
milestone: "0.4.0"
status: completed
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

One loop invocation is represented by one durable run manifest with N tagged
sub-runs. The main checkout owns the manifest and transcript files under
`.loopdeck/agent-runs/<run-id>/`; individual agents never share a checkout or
write the main tree's loop memory. Each assigned profile is resolved once,
including its UUID-scoped secret, before background execution starts.

The spike confirmed the safe split: serialize linked-worktree creation and
manifest transitions with a project-scoped async lock, then run provider turns
concurrently with `JoinSet`. Each agent sees only its own branch/worktree.
Tests hold two sub-run tasks concurrently, verify their files are isolated, and
remove both pristine worktrees. Successful modified trees and failed/cancelled
trees are retained; only pristine successful trees are automatically removed.

Lifecycle state is durable and restart-safe. Startup reconciles stale queued,
running, or waiting sub-runs to cancelled. A project-scoped admission guard
prevents overlapping logical runs and retry races; interrupt state is persisted
before the provider signal and cannot be overwritten by a late result. UI
events carry both run ID and agent ID, and each sub-run renders its own output,
status, questions, permissions, and plan approvals.

## Phases

### Phase 1 — Spike: worktree isolation under N concurrent sessions

- [x] Prototype spawning 2-3 `HarnessSession`s concurrently, each against a `worktree_add`-created worktree, and confirm no shared-state write races (particularly `.loopdeck/loops.md` on the main tree); write findings back into this PRD's Design section

### Phase 2 — Per-loop agent assignment (backend + IPC)

- [x] Extend the loop-start path (`agent.rs:1641` and callers) to accept a list of named config references instead of resolving the single global config

### Phase 3 — Concurrent execution engine

- [x] Spawn one `HarnessSession` per assigned config inside its own `worktree_add`-created worktree, running concurrently; clean up via `worktree_remove` on completion per the spike's findings

### Phase 4 — Per-agent output/transcript tagging (frontend)

- [x] Tag each session's transcript/output with its source config name in `src/components/`, so the loop view distinguishes concurrent agents' output

### Phase 5 — Tests/verification

- [x] Rust tests for concurrent spawn/cleanup; verify against prd-agent-config's roster once merged

## Open Questions

- **Resolved:** one logical run owns N tagged sub-runs. `.loopdeck/loops.md`
  remains on the existing main-tree/single-writer path.
- **Resolved:** every result remains independently visible under its named
  profile. Comparison, winner selection, and merge behavior remain explicit
  non-goals.
