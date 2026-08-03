---
prd: prd-agent-config
epic: multi-model-agents
milestone: "0.4.0"
status: completed
description: >
  Turn the single global AgentConfig into a named, storable roster with CRUD
  and persistence, so a user can define "Opus", "Codex", and
  "Claude — staging URL" once and reuse each across loops.
---

## Overview

`src-tauri/src/config.rs:22` defines `AgentConfig` (harness, base_url, model,
auth_token, effort) and `resolve_agent_config`
(`src-tauri/src/commands/state.rs:170`) resolves exactly one instance per
call. This PRD adds a name to that shape, stores a roster of named entries
instead of one value, and exposes CRUD over that roster through
`config_cmds.rs` and the frontend.

## Problem Statement

A user cannot save more than one agent configuration. Switching between
"Claude on Opus" and "Codex" today means overwriting the single global
config and re-saving, which makes running distinct agents side by side
impossible and loses the previous config's values.

## Goals

| Priority | Goal |
|---|---|
| P0 | Named `AgentConfig` entries stored in a roster, persisted in existing config storage |
| P0 | Create/read/update/delete a named entry via IPC commands |
| P1 | Roster CRUD UI (list, add, edit, delete) |
| P2 | Migration path from the existing single global `AgentConfig` into the first roster entry |

## Non-Goals

- Per-loop assignment UI/logic (covered by prd-multi-agent-execution).
- Concurrent execution (covered by prd-multi-agent-execution).

## Design

The roster is global and stored alongside the existing project registry in
`~/.config/loopdeck/config.yaml`; projects only reference immutable profile
IDs. `NamedAgentConfig` flattens the existing `AgentConfig` fields beside
`id` and `name`, and `default_agent_id` preserves the single-agent default.

Credentials never enter YAML or IPC responses. Each profile has an owner-only
secret file addressed by UUID, while responses expose only
`has_auth_token`. The legacy singleton and legacy secret are migrated to a
reserved deterministic UUID, making the migration idempotent even if startup
is interrupted. CRUD commands snapshot registry and secret state and restore
both if persistence fails.

## Phases

### Phase 1 — Config data model

- [x] Add a `name` field and roster container (e.g. `Vec<NamedAgentConfig>`) alongside the existing `AgentConfig` in `src-tauri/src/config.rs`, preserving the current single-config fields per entry

### Phase 2 — Storage + persistence

- [x] Extend `GlobalConfig` load/save to persist the roster, with a migration step that wraps any existing single `AgentConfig` into the first named entry

### Phase 3 — Config CRUD IPC + UI

- [x] Add `list_agent_configs` / `create_agent_config` / `update_agent_config` / `delete_agent_config` commands in `src-tauri/src/commands/config_cmds.rs`
- [x] Build the roster list/add/edit/delete UI in `src/components/`, wired through typed IPC in `src/lib/tauri.ts`

### Phase 4 — Tests/verification

- [x] Rust unit tests for roster persistence and migration in `config.rs`; frontend test for CRUD roundtrip

## Open Questions

- **Resolved:** the roster is global; run manifests keep per-project assignment
  snapshots by immutable profile ID.
- **Resolved:** secrets are isolated one file per profile UUID; names are never
  used as credential keys.
