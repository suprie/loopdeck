# PRD — Global Agent Configuration

## Status: 📋 Proposed (2026-06-30)

---

## Overview

Move agent configuration (API provider, model, auth token, effort level) from
hardcoded constants in `src-tauri/src/agents.rs` into the global config file at
`~/.config/loopdeck/config.yaml`. One agent config for all LoopDeck-tracked
projects — simple, single-provider, single-key.

Per-project overrides are deferred to a future release.

---

## Problem Statement

Currently `call_agents()` in `agents.rs` hardcodes **8 environment variables**
directly in Rust source:

- `ANTHROPIC_AUTH_TOKEN` — plaintext API key committed to git
- `ANTHROPIC_BASE_URL` — provider endpoint
- `ANTHROPIC_MODEL` / `ANTHROPIC_DEFAULT_SONNET_MODEL` / etc. — model selection
- `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, `CLAUDE_CODE_EFFORT_LEVEL`

This creates three problems:

1. **Security** — API key is in source code, will be committed to git
2. **No persistence** — configuration is lost on rebuild; users must edit Rust
   code to change settings
3. **No discoverability** — users have no UI to configure agents; they must
   read and edit Rust source

---

## Goals

| Priority | Goal |
|----------|------|
| P0 | Store agent config in `~/.config/loopdeck/config.yaml` (global, not per-project) |
| P0 | Zero hardcoded secrets — auth token read from config file, never from source |
| P1 | `call_agents()` reads config from `GlobalConfig` instead of literals |
| P1 | IPC command to read/write agent config from frontend |
| P2 | Settings page in UI to edit agent config (global app settings, not per-project) |

## Non-Goals

- Per-project agent overrides (future — add `agent:` to `ProjectEntry` or
  `.loopdeck/project.yaml` later)
- Multi-provider routing (future)
- Agent orchestration / multi-agent (future)
- Credential encryption at rest (future — delegate to OS keychain)

---

## Proposed Schema

### `~/.config/loopdeck/config.yaml` (extended)

```yaml
settings:
  scan_depth: 5

# NEW: global agent configuration block (optional)
agent:
  auth_token: sk-abc123...          # required to run agents
  base_url: https://api.anthropic.com   # optional, defaults to Anthropic
  model: claude-sonnet-4-6          # optional
  effort: max                       # optional, one of: low | medium | high | max

projects:
  - path: /Users/alice/work/my-app
    name: my-app
    # ...
```

### Rust struct changes

```rust
// config.rs — added to GlobalConfig
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    pub auth_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

// GlobalConfig gains an optional field
pub struct GlobalConfig {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentConfig>,
}
```

### Design rationale: global-first, per-project later

- Most developers use **one** API key and **one** preferred provider
- Per-project model selection is an edge case (Rust project wants Opus,
  Python project wants Sonnet) — YAGNI for now
- If per-project overrides are needed later, add `agent: Option<AgentConfig>`
  to `ProjectEntry` that merges with the global default (project > global >
  built-in default)
- This matches tools like GitHub Copilot, Cursor, etc. — global settings,
  with per-workspace overrides as an advanced feature

### IPC commands (net-new)

| Command | Args | Returns | Description |
|---------|------|---------|-------------|
| `get_agent_config` | _(none)_ | `AgentConfig \| null` | Read agent config from global config.yaml |
| `set_agent_config` | `config: AgentConfig` | `AgentConfig` | Write (or update) global agent config |

No `repo_path` parameter needed — global config lives at a fixed path.

---

## Implementation Plan

### Phase 1 — Backend (rust-expert)

1. Move `AgentConfig` from `agents.rs` to `config.rs` (single source of truth)
2. Add `Serialize`, `Deserialize` derives; implement `Default` manually (empty
   fields, no default token)
3. Add `agent: Option<AgentConfig>` field to `GlobalConfig`
4. Add `get_agent_config` and `set_agent_config` to `commands.rs`
   - `get_agent_config` loads `GlobalConfig`, returns `config.agent`
   - `set_agent_config` loads, sets `config.agent = Some(new_config)`, saves
5. Refactor `call_agents()` to accept `&AgentConfig` as a parameter and set env
   vars from it (remove the 8 hardcoded `cmd.env(...)` calls)
6. Add unit tests for:
   - Loading config.yaml with agent block
   - Loading config.yaml without agent block (backward compat)
   - `set_agent_config` round-trip (write → read)
   - `call_agents` sets correct env vars from `AgentConfig`

### Phase 2 — Frontend (vite-senior-engineer)

1. Add `AgentConfig` to TypeScript types (`src/types/index.ts`)
2. Add typed IPC wrappers (`getAgentConfig`, `setAgentConfig`)
3. Add Settings page (or section) with agent config form
4. Form fields: `auth_token` (password), `base_url`, `model` (text input +
   common presets), `effort` (select: low/medium/high/max)
5. Save button calls `set_agent_config`

### Phase 3 — Security hardening (future)

- Read `auth_token` from environment variable reference (e.g., `${OPENAI_KEY}`)
- OS keychain integration via `keyring` crate
- Never echo `auth_token` in logs or debug output

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| API key in plaintext YAML | Config file at `~/.config/loopdeck/` is user-private (not in a git repo). Future: OS keychain. |
| Breaking existing `config.yaml` files | `Option<AgentConfig>` with serde `default` + `skip_serializing_if` = backward compatible; existing configs load as `agent: None` |
| Token leak via debug prints | `AgentConfig` must implement a manual `Debug` that redacts `auth_token` |
| State contention | `GlobalConfig` is wrapped in `Mutex` in Tauri state — serialized access is already enforced |

---

## Success Criteria

- [ ] `call_agents()` takes an `&AgentConfig` parameter — zero hardcoded env vars
- [ ] Existing `~/.config/loopdeck/config.yaml` files without `agent:` still load correctly
- [ ] `set_agent_config` writes valid YAML that survives round-trip (write → read)
- [ ] All existing tests pass unchanged (backward compatibility)
- [ ] New tests cover: config with agent, config without agent, round-trip save/load
- [ ] Frontend can display and edit agent config in a Settings view
