---
prd: prd-non-claude-adapter
epic: multi-model-agents
milestone: "0.4.0"
status: proposed
description: >
  Extract a pluggable adapter interface from the existing Claude/Codex
  session split, proven with the already-present Codex CLI adapter, so a
  non-Claude-Code agent is invoked and captured the same way as a Claude
  session.
---

## Overview

`src-tauri/src/harness.rs` already dispatches on `AgentHarness`
(`Claude`/`Codex`, `config.rs:11`) between `claude_session.rs` and
`codex_session.rs`. This PRD formalizes that split into an explicit adapter
interface so the multi-agent execution engine (prd-multi-agent-execution)
can spawn either harness uniformly per assigned config, and documents the
Codex adapter as the reference implementation for any future non-Claude
adapter.

## Problem Statement

The Claude/Codex split today lives as harness-specific branching in
`harness.rs` rather than a named interface. As the execution engine starts
spawning N sessions of potentially mixed harnesses concurrently, an
explicit adapter boundary makes it clear what a harness must implement
(spawn, stream, respond to control requests) without the caller needing to
know which harness it's talking to.

## Goals

| Priority | Goal |
|---|---|
| P0 | An explicit adapter trait/interface capturing spawn + stream + control-request handling, implemented by both Claude and Codex sessions |
| P0 | `harness.rs` dispatches through the interface rather than harness-specific branches |
| P1 | Adapter-level test coverage confirming both harnesses satisfy the same contract |

## Non-Goals

- New adapters beyond Claude and Codex (epic non-goal).
- Changing Claude or Codex session behavior — this is an extraction, not a rewrite.

## Design

<!-- Stub — fill in during implementation. Key open question: async trait
object (dyn-compatible) vs. an enum dispatch that stays closer to the
current AgentHarness match — pick based on how prd-multi-agent-execution's
concurrent spawn needs to hold a heterogeneous list of sessions. -->

## Phases

### Phase 1 — Adapter interface

- [ ] Define the adapter trait (spawn, stream, control-request handling) in `src-tauri/src/harness.rs`, derived from the current `HarnessSession::spawn` signature shared by `claude_session.rs` and `codex_session.rs`

### Phase 2 — Wire existing Claude + Codex sessions through it

- [ ] Refactor `claude_session.rs` and `codex_session.rs` to implement the adapter interface, and update `harness.rs` dispatch to go through it instead of matching `AgentHarness` directly

### Phase 3 — Tests/verification

- [ ] Adapter-contract tests confirming both Claude and Codex sessions satisfy the same interface; confirm no behavior change via existing `claude_session.rs`/`codex_session.rs` test suites still passing

## Open Questions

- Trait-object vs. enum dispatch — decide based on prd-multi-agent-execution's concurrent-spawn shape once its spike (Phase 1) lands.
