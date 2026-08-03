---
title: Multi-Model Agent Runner
slug: multi-model-agents
milestone: "0.4.0"
status: completed
started: 2026-08-03
owner: Suprie
description: >
  Let a user assign a distinct model, provider, and connection config to each
  agent in a loop — for example Agent A on Claude Opus, Agent B on Codex, and
  Agent C on Claude against a different environment URL — and run them
  concurrently, so loops are no longer locked to the single global agent
  config LoopDeck resolves today.
---

## Motivation

Today `resolve_agent_config` (`src-tauri/src/commands/state.rs:170`) resolves
exactly one global `AgentConfig` (harness, model, base URL, auth token,
effort) for every loop, and every loop spawns exactly one `HarnessSession`.
A user who wants to compare how Claude Opus, Codex, and a Claude session
against a custom environment URL each handle the same task has no way to do
that inside LoopDeck — they'd have to reconfigure the single global config
and re-run the loop three times, serially, by hand.

This epic turns the single global config into a reusable, named roster, lets
a user assign one or more roster entries to a loop, and runs the assigned
agents concurrently — each in its own isolated worktree — with output kept
distinguishable per agent in the UI.

## Scope

- Agent config schema (provider, model, base URL, auth) — extends the
  existing `AgentConfig` (`src-tauri/src/config.rs:22`) from a single value
  into a named, storable entry.
- Config CRUD + persistence (UI + storage) — create/edit/delete named agent
  configs, saved and reusable across loops (extends
  `get_agent_config`/`set_agent_config` in
  `src-tauri/src/commands/config_cmds.rs`).
- Per-loop agent assignment — a loop is started with one or more named
  configs instead of the single resolved global config.
- Concurrent multi-agent execution engine — spawns N `HarnessSession`s in
  parallel, one per assigned config, each isolated via the existing
  `worktree_add`/`worktree_remove` machinery (`src-tauri/src/git.rs:44`,
  `:76`) built for overnight-orchestration.
- Per-agent output/transcript tagging in UI — loop view distinguishes output
  by which named config produced it.
- Non-Claude adapter (e.g. Codex CLI) — a pluggable adapter layer, proven
  against the existing `codex_session.rs`, so a non-Claude-Code agent is
  invoked and captured the same way as a Claude session.

## Non-Goals

- Result comparison/merge UI — no side-by-side diff or voting UI to pick a
  winning agent's output; this epic only runs agents in parallel.
- New non-Claude adapters beyond one example — only the Codex adapter is
  built to prove the pattern; a general adapter marketplace is out of scope.

## PRD Index

| PRD | Description | Status |
|---|---|---|
| [prd-agent-config](prd-agent-config.md) | Named agent config roster: schema, CRUD, persistence | completed |
| [prd-multi-agent-execution](prd-multi-agent-execution.md) | Per-loop assignment, concurrent execution engine, per-agent output tagging | completed |
| [prd-non-claude-adapter](prd-non-claude-adapter.md) | Adapter trait proven with a Codex CLI adapter | completed |

## Architecture Decisions

### ADR-1: Global UUID roster with UUID-scoped secrets

Named profiles live in the existing global registry because they are reusable
across projects. Assignments and secrets use immutable UUIDs, so renaming a
profile cannot redirect credentials or break a saved run. The legacy singleton
is migrated to a deterministic default entry and legacy IPC remains as a shim.

### ADR-2: One durable logical run with isolated tagged sub-runs

A multi-agent invocation owns one manifest under
`.loopdeck/agent-runs/<run-id>/`, with one tagged sub-run per assigned profile.
Each sub-run gets its own linked worktree and branch. Git worktree creation and
manifest transitions are serialized per project, while provider turns run
concurrently. Pristine successful worktrees are removed; modified or failed
worktrees are retained as inspectable artifacts.

### ADR-3: Shared adapter trait with enum dispatch

Claude and Codex implement the same crate-private `HarnessAdapter` contract.
`HarnessSession` remains the heterogeneous enum held by callers and delegates
through that contract, avoiding async trait objects while preserving one
stream/control surface for both providers.

## Success Criteria

- Agent configs persist and are reusable across loops.
- Loop view output/transcripts are distinguishable by which agent config
  produced them.
- Two to eight assigned agents can run concurrently without sharing a checkout.
- Run manifests survive restart; stale queued or active sub-runs become
  retryable cancelled records rather than blocking future work.

## Risks

| Risk | Mitigation |
|---|---|
| Concurrent execution safety — multiple agents running against the same repo/worktree risk file conflicts or resource contention | Reuse the existing linked-worktree isolation (`git.rs` `worktree_add`/`worktree_remove`) built for overnight-orchestration runs: each concurrent agent gets its own worktree and branch, so no two agents write the same checkout. Shared state outside the worktree (e.g. `.loopdeck/loops.md` on the main tree) stays on the existing single-writer path. Prd-multi-agent-execution opens with a spike phase to confirm this holds under N concurrent sessions before the execution engine is built — user to confirm/adjust mitigation. |
