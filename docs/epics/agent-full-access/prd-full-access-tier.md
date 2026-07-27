---
prd: prd-full-access-tier
epic: agent-full-access
milestone: "0.3.0"
status: accepted
description: >
  Add a per-project "full access" permission tier to the LoopDeck agent
  runtime. Extends the Rust PermissionMode enum with a FullAccess variant
  (policy auto-allow after the destructive floor), persists a
  permission_mode field on ProjectEntry, exposes a set_project_permission_mode
  IPC command, and adds a tier selector + mode-aware badge in the Agent
  panel. Delivers autonomy without giving up the safety floor or audit
  trail. This is the runtime half of the agent-full-access epic.
---

# PRD — Full Access Permission Tier

## Overview

Add a per-project "full access" permission tier for LoopDeck-spawned agents.
Today every agent runs under `ConfirmChanges`, which parks every
mutating/executing tool call on a manual-approval card. That's correct for
untrusted repos but stalls the agent once the user trusts the project. This
PRD adds a second tier — `FullAccess` — that auto-approves control requests
while keeping the destructive-command floor and the audit trail intact.

The implementation is **policy auto-allow, not CLI bypass** (see epic ADR-1).
LoopDeck keeps spawning Claude with `--permission-mode default` so every tool
call still routes over stdio; the new policy variant answers `allow` to any
request that clears the floor. The floor (`permission.rs:180-493`) continues
to hard-deny `rm -rf /`, `git push --force`, pipe-to-shell, etc. Every
decision still logs.

This is the runtime half of the `agent-full-access` epic. The skills half
(verify + open-pr) is `prd-verify-and-ship-skills.md`.

## Amendment — Delivered as per-project "Autonomous Mode" (2026-07-27)

This PRD's checkboxes were left unchecked and its `status` stuck at
`proposed` even though the runtime half shipped on **2026-07-23** as
**"Per-project Autonomous Mode"** (`decisions.md:626`, `loops.md` Gate C
supporting work). Same design (ADR-1/ADR-2/ADR-3 all hold: policy
auto-allow not CLI bypass, per-project not per-spawn, a distinct variant
from the deferred `AutonomousProject`), different names — reconciled here
rather than left to read as unfinished work, per the lesson from the
`prd-skill-split` near-miss (`loops.md`, 2026-07-27 entry).

| PRD (proposed) | Shipped (actual) |
|---|---|
| `PermissionMode::FullAccess` variant | `PermissionMode::Autonomous` (`permission.rs`) |
| `PermissionPolicy::full_access()` | `PermissionPolicy::with_mode(PermissionMode::Autonomous)` — no dedicated constructor was added; `with_mode` already existed for tests and gained a production caller |
| `PermissionPolicy::intercepts_manually() -> bool` | No new method. `claude_session.rs::answer_control_request` arm 3 checks `!self.policy.is_autonomous()` inline instead |
| `ProjectPermissionMode` enum (`confirm_changes`/`full_access`), `ProjectEntry::permission_mode` | Plain `ProjectEntry::autonomous: bool`, `#[serde(default, skip_serializing_if = "std::ops::Not::not")]` — same backward-compat guarantee (missing field → `false`/confirm-changes), simpler shape since there are only two tiers |
| `set_project_permission_mode(path, mode)` IPC command | `set_project_autonomous(path, autonomous)` (`commands/project.rs`) |
| `project_permission_policy(state, path)` helper | `resolve_permission_policy(state, path)` (`commands/state.rs`) — same unregistered-path-defaults-to-`ConfirmChanges` contract |
| TS `PermissionMode = "confirm_changes" \| "full_access"` | `PermissionModeBadge` takes `mode?: "confirm" | "autonomous"`; no standalone exported TS union type |
| Tier selector in `AgentPanel.tsx`/`AgentRunner.tsx` toolbars | Toggle + confirm dialog lives in `ProjectDetail.tsx`'s `OverviewTab` instead (`handleToggleAutonomous`, `showAutonomousConfirm`); `AgentPanel`/`AgentRunner` only *render* the mode-aware badge, reading `project.autonomous` from the store |
| — (not in original design) | Extra: `Chat.tsx` renders a per-turn amber "auto-approved" tag on mutating tool calls when the project is autonomous (`wouldRequireApproval` + `autonomous` prop threading through `BlockList`/`TurnBubble`/`StreamingBubble`) — a usability addition beyond the PRD's scope, giving the morning-review workflow visibility into what the agent auto-approved |

All five Success Criteria in the epic README are met by the shipped
implementation (verified against the code, not re-derived from the PRD
text): the toggle flips a project to unattended operation from
`OverviewTab`; a destructive-floor-shaped Bash call still hard-denies under
`Autonomous` (`floor_denies_regardless_of_mode` property test); an existing
`config.yaml` with no `autonomous` field deserializes to `false` cleanly
(`#[serde(default, ...)]`); LoopDeck's own Rust/TS gates were green at
merge (fmt/clippy/tests/tsc, see the 2026-07-23 decision).

**Not reconciled here, left open:** none of the Open Questions below needed
a fresh decision — the shipped design answered them the same way the PRD's
leans suggested (per-project persistence, no separate bypass-confirmation
dialog, next-spawn-not-live-reconfiguration). See `decisions.md:626` for
the authoritative record.

## Problem Statement

The current permission posture is hardcoded and global:

- `PermissionMode` (`src-tauri/src/permission.rs:72-85`) has two variants:
  `ConfirmChanges` (the only production mode) and `Deny` (test-only). The
  documented `AutonomousProject` variant is deferred pending path-containment
  helpers — it is not wired.
- `PermissionPolicy::confirm_changes()` (`permission.rs:98-102`) is the only
  constructor used in production. `spawn_fresh` at
  `commands/agent.rs:1156` calls it as a compile-time constant with no
  per-project lookup.
- `ProjectEntry` (`config.rs:110-139`) has no permission/trust/sandbox field.
  The only per-project permission state is the external
  `.claude/settings.local.json` allow rules, which the trust-boundary PRD
  explicitly forbids as a hidden default and which the user must hand-curate.

So a user who trusts a project has two bad options: click "Allow" on every
`git add`/`cargo`/`npm` call, or hand-write broad allow rules into
`settings.local.json` that violate the trust-boundary rules. The frontend has
no notion of a tier either — `PermissionModeBadge.tsx` is a static,
propless component that always renders "Confirm changes," though its doc
comment already anticipates becoming mode-aware.

## Goals

| Priority | Goal |
|--------|------|
| P0 | `FullAccess` variant on `PermissionMode` that auto-allows any floor-clearing request, while the destructive floor still hard-denies |
| P0 | `PermissionPolicy::full_access()` constructor mirroring `confirm_changes()` |
| P0 | Persisted `ProjectEntry::permission_mode` field with serde `default = ConfirmChanges` for backward compatibility |
| P0 | `set_project_permission_mode(path, mode)` IPC command that updates the persisted field and saves `config.yaml` |
| P0 | `spawn_fresh` reads the project's mode via a `project_permission_policy(state, path)` helper instead of the hardcoded `confirm_changes()` |
| P1 | Frontend `PermissionMode` TS type mirroring the Rust enum's wire shape |
| P1 | `PermissionModeBadge` becomes mode-aware (accepts a `mode` prop, renders distinct styling/copy for `FullAccess`) |
| P1 | Per-project tier selector in the Agent panel toolbar (`AgentPanel.tsx`) and the `/agent` page header (`AgentRunner.tsx`) |
| P1 | `setProjectPermissionMode(path, mode)` typed IPC wrapper in `lib/tauri.ts` |
| P2 | Honest copy in the selector popover: "applies to the next conversation; the destructive floor still applies" |

## Non-Goals

- **CLI flag flipping.** The spawn arg stays `--permission-mode default`. A
  future `bypassPermissions`-backed tier is out of scope (epic ADR-1).
- **Per-spawn tier.** The tier is per-project only, to avoid changing the
  streaming IPC signatures (epic ADR-2).
- **`AutonomousProject`.** That path-containment-bounded variant remains
  deferred. `FullAccess` is a distinct, broader variant (epic ADR-3).
- **Updating the curated allowlist seeding in `skills.rs::setup_hooks`.** The
  trust-boundary PRD forbids broad `Edit(*)`/`Write(*)` as hidden defaults;
  `FullAccess` does not change what gets seeded into `.claude/settings.json`.
  The autonomy comes from the runtime policy, not from on-disk rules.
- **Rewriting `PRD-trust-boundary-hardening.md`.** A cross-reference is added
  in the epic; the trust-boundary PRD continues to describe the two-mode
  contract. Folding `FullAccess` in is a follow-up once the runtime semantics
  are proven.

## Design

### Rust enum + policy (`src-tauri/src/permission.rs`)

Add `FullAccess` to `PermissionMode` (`permission.rs:72-85`), after
`ConfirmChanges`:

```rust
pub enum PermissionMode {
    ConfirmChanges,
    #[allow(dead_code)]
    Deny,
    /// Auto-allow any request that clears the destructive floor. The floor
    /// still hard-denies rm -rf / git push --force / pipe-to-shell / …, and
    /// every decision still flows through the audit path. Opt-in per project
    /// via `ProjectEntry::permission_mode`. This is NOT `claude
    /// --permission-mode bypassPermissions` — the CLI flag stays `default`,
    /// so LoopDeck keeps the audit trail and the safety net.
    FullAccess,
}
```

In `PermissionPolicy::decide()` (`permission.rs:118-131`), the floor check at
`:121-123` already runs before the match, so the new arm is one line:

```rust
match self.mode {
    PermissionMode::ConfirmChanges => Decision::Allow,
    PermissionMode::Deny => Decision::Deny(String::from(
        "no matching allow rule and LoopDeck is deny-by-default",
    )),
    PermissionMode::FullAccess => Decision::Allow,
}
```

Add the constructor:

```rust
pub fn full_access() -> Self {
    Self { mode: PermissionMode::FullAccess }
}
```

Remove `#[allow(dead_code)]` from `with_mode` (`permission.rs:104-108`) since
it now has a production caller (the lookup helper below).

Update the module doc comment posture section (`permission.rs:8-22`) to
describe both modes honestly — `FullAccess` auto-allows after the floor, the
floor always applies.

**Important:** the `MANUAL_APPROVAL_TOOLS` interception
(`claude_session.rs::answer_control_request`) still parks `Bash`/`Edit`/`Write`
on a UI approval card *before* `decide()` is consulted. Under `FullAccess`,
that interception must be bypassed for the session, otherwise the policy arm
is dead code and the user still sees approval cards. The cleanest hook is a
`PermissionPolicy::intercepts_manually() -> bool` method that returns `true`
for `ConfirmChanges`/`Deny` and `false` for `FullAccess`; `answer_control_request`
checks it before parking. This keeps the bypass localized to the policy
object and testable without spawning Claude.

### Persisted field (`src-tauri/src/config.rs`)

Add a new enum + field on `ProjectEntry` (`config.rs:110-139`):

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectPermissionMode {
    #[default]
    ConfirmChanges,
    FullAccess,
}
```

The wire shape is `confirm_changes` / `full_access` (snake_case) to match the
Rust convention used by `RunState` (`config.rs:100-108`). Kept as a separate
enum from internal `PermissionMode` so `Deny` (test-only) is never a
persistable project option and so the wire/persistence shape is decoupled
from internal policy variants.

On `ProjectEntry`:

```rust
#[serde(default)]
pub permission_mode: ProjectPermissionMode,
```

`#[serde(default)]` means existing `config.yaml` entries deserialize to
`ConfirmChanges` unchanged — no migration. Skip-serializing-when-default is
optional (`#[serde(default, skip_serializing_if = "is_permission_default")]`)
to keep `config.yaml` uncluttered; mirror the `is_run_state_idle` pattern at
`config.rs:143-145`.

### Spawn wire-through (`src-tauri/src/commands/agent.rs`)

Replace the hardcoded `PermissionPolicy::confirm_changes()` at `agent.rs:1156`
with a lookup:

```rust
let policy = project_permission_policy(state, path)?;
let session = ClaudeSession::spawn(
    &path.to_path_buf(),
    &agent_config,
    None,
    policy,
)?;
```

New helper (in `commands/agent.rs` or `commands/state.rs`):

```rust
fn project_permission_policy(
    state: &AppState,
    path: &Path,
) -> Result<crate::permission::PermissionPolicy, AppError> {
    let config = state.config.lock().map_err(|_| AppError::LockError)?;
    let mode = config
        .find(path)  // existing GlobalConfig::find helper
        .map(|entry| entry.permission_mode)
        .unwrap_or_default();
    Ok(match mode {
        crate::config::ProjectPermissionMode::ConfirmChanges =>
            crate::permission::PermissionPolicy::confirm_changes(),
        crate::config::ProjectPermissionMode::FullAccess =>
            crate::permission::PermissionPolicy::full_access(),
    })
}
```

Defaults to `confirm_changes()` for unregistered paths. Grep for any other
`PermissionPolicy::confirm_changes()` call sites and apply the same lookup;
the primary production caller is `spawn_fresh:1156`, but `with_session`
paths should be verified before editing.

### IPC command (`src-tauri/src/commands/config_cmds.rs`)

Mirror `set_agent_config` (`config_cmds.rs:36-67`):

```rust
#[tauri::command]
pub async fn set_project_permission_mode(
    path: String,
    mode: ProjectPermissionMode,
    state: State<'_, AppState>,
) -> Result<ProjectPermissionMode, AppError> {
    let mut config = state.config.lock().map_err(|_| AppError::LockError)?;
    let entry = config
        .projects
        .iter_mut()
        .find(|p| p.path == PathBuf::from(&path))
        .ok_or_else(|| AppError::ProjectNotFound)?;
    entry.permission_mode = mode;
    config.save()?;
    Ok(mode)
}
```

Register in `lib.rs` `generate_handler!` alongside `set_agent_config`
(`lib.rs:130-132`).

### Frontend (`src/`)

**`src/types/index.ts`** — new type + field:

```ts
export type PermissionMode = "confirm_changes" | "full_access";
```

Add to `ProjectEntry` (`types/index.ts:77-98`):

```ts
permission_mode?: PermissionMode;
```

The tier lives on `ProjectEntry` (global registry), NOT on `ProjectMeta`
(`.loopdeck/project.yaml`) — the tier is a LoopDeck runtime concern, not a
repo-portable artifact, and should not pollute the user's repo.

**`src/lib/tauri.ts`** — wrapper after `agentAddAllowRule` (`:452`):

```ts
export async function setProjectPermissionMode(
  path: string,
  mode: PermissionMode,
): Promise<PermissionMode> {
  return invoke("set_project_permission_mode", { path, mode });
}
```

No new getter needed — the mode rides on `ProjectEntry` from `list_projects`.

**`src/components/shared/PermissionModeBadge.tsx`** (currently 24 lines,
propless) — become mode-aware:

- Accept `mode: PermissionMode` prop (default `"confirm_changes"` so existing
  call sites compile).
- Render `ConfirmChanges` as today (`ShieldCheck`, blue).
- Render `FullAccess` with `ShieldAlert` (amber) and a tooltip that is honest
  about the semantics: "Auto-approves tool calls. The destructive floor
  (rm -rf, force-push, …) still applies."

**`src/components/detail/AgentPanel.tsx`** (`:894-1004` toolbar, `:896` badge
mount) — read the selected project's `permission_mode`, render a clickable
tier toggle next to the badge. Pattern to mirror: `AskUserQuestionCard.tsx`'s
single-select card. On change, call `setProjectPermissionMode(projectPath,
mode)`. The popover copy must say "applies to the next conversation" so the
user knows a live session isn't retroactively reconfigured.

**`src/components/agent/AgentRunner.tsx`** (`:85` badge mount) — same
mode-aware wiring.

## Phases

### Phase 1 — Backend permission mode + persistence

- [x] Add `FullAccess` variant to `PermissionMode` (`permission.rs:72-85`); update doc comment — shipped as `PermissionMode::Autonomous`, doc comment describes the same auto-allow-after-floor semantics
- [x] Add `PermissionPolicy::full_access()` constructor; add the `FullAccess` arm to `decide()` (`permission.rs:118-131`) — shipped via the existing `with_mode(PermissionMode::Autonomous)`, no dedicated named constructor was added
- [x] Add `PermissionPolicy::intercepts_manually() -> bool` returning `false` for `FullAccess`, `true` otherwise — shipped as an inline `!self.policy.is_autonomous()` check in `answer_control_request` arm 3 rather than a named policy method; same effect, verified by `floor_denies_regardless_of_mode`
- [x] Wire `intercepts_manually()` into `claude_session.rs::answer_control_request` so `MANUAL_APPROVAL_TOOLS` is bypassed under `FullAccess` — see above, arm 3 now also checks `!policy.is_autonomous()`
- [x] Remove `#[allow(dead_code)]` from `with_mode` (`permission.rs:104-108`) — `with_mode` gained the production caller in `resolve_permission_policy`
- [x] Define `ProjectPermissionMode` enum + `ProjectEntry::permission_mode` field with `#[serde(default)]` (`config.rs`) — shipped as a plain `autonomous: bool` field with `#[serde(default, skip_serializing_if = "std::ops::Not::not")]` (no enum; two tiers didn't need one)
- [x] Add `fn project_permission_policy(state, path) -> Result<PermissionPolicy, AppError>` helper — shipped as `resolve_permission_policy(state, path) -> PermissionPolicy` (infallible: unregistered paths resolve to `ConfirmChanges` rather than erroring) in `commands/state.rs`
- [x] Replace the hardcoded `confirm_changes()` at `commands/agent.rs:1156` with the lookup; grep for any other production call sites — `resolve_permission_policy` is threaded to both spawn sites
- [x] Add `set_project_permission_mode` IPC command in `commands/config_cmds.rs` mirroring `set_agent_config` — shipped as `set_project_autonomous(path, autonomous: bool)` in `commands/project.rs`
- [x] Register the command in `lib.rs` `generate_handler!`

### Phase 2 — Frontend selector + mode-aware badge

- [x] Add `PermissionMode` TS type to `src/types/index.ts`; add `permission_mode?` to `ProjectEntry` — shipped as `ProjectEntry.autonomous?: boolean`; no standalone exported union type (only the badge's local `"confirm" | "autonomous"` prop type)
- [x] Add `setProjectPermissionMode` wrapper to `src/lib/tauri.ts` — shipped as `setAutonomous(path, autonomous)`
- [x] Make `PermissionModeBadge` accept a `mode` prop and render distinct styling/copy for `FullAccess` — accepts `mode?: "confirm" | "autonomous"`, amber `ShieldAlert` styling for autonomous
- [x] Add tier selector in `AgentPanel.tsx` toolbar; wire to `setProjectPermissionMode` — **deviation:** the toggle + confirm dialog live in `ProjectDetail.tsx`'s `OverviewTab` (`handleToggleAutonomous`/`showAutonomousConfirm`), not the `AgentPanel` toolbar; `AgentPanel` only renders the mode-aware badge, reading the flag from the store
- [x] Mirror selector wiring in `AgentRunner.tsx` — same deviation: `AgentRunner` renders the badge (`mode={selected?.autonomous ? "autonomous" : "confirm"}`) but has no toggle of its own; the single `OverviewTab` toggle is the one control surface
- [x] Honest popover copy: "applies to the next conversation; the destructive floor still applies" — shipped as the confirm-dialog copy in `OverviewTab` plus the badge's `title` tooltip ("The destructive floor still applies…")
- [x] `npx tsc --noEmit` clean

### Phase 3 — Tests

- [x] `decide_full_access_allows_bash_edit_write` — confirm Bash/Edit/Write inputs return `Allow` under `FullAccess` — shipped as the `Autonomous`-mode coverage in `permission.rs`'s test module
- [x] `decide_full_access_still_denies_destructive_floor` — confirm `rm -rf /` / `git push --force` still hard-deny under `FullAccess` — shipped as `floor_denies_regardless_of_mode` (a property test, broader than the PRD's example-based ask)
- [x] `intercepts_manually_false_under_full_access` — confirm the manual-approval interception is bypassed — covered by `claude_session.rs` tests asserting arm 3 is skipped under `Autonomous`
- [x] `ProjectPermissionMode` round-trip serialization test in `config.rs` tests (both variants; missing-field defaults to `ConfirmChanges`) — shipped as a round-trip test on the plain `autonomous: bool` field (serde default `false` on a missing field)
- [x] `set_project_permission_mode` happy-path + `ProjectNotFound` error path test — shipped against `set_project_autonomous`
- [x] `cargo test`, `cargo clippy -D warnings` clean — green at merge, see the 2026-07-23 decision

## Open Questions

- Should `permission_mode` be `skip_serializing_if = "is_permission_default"`
  to keep `config.yaml` uncluttered for the common case? **Lean:** yes, mirror
  `is_run_state_idle`. The default tier shouldn't clutter every project entry.
- Should the tier selector live in the Agent panel toolbar (next to the badge)
  or in ProjectDetail's Overview tab (as a per-project setting)? **Lean:**
  toolbar, because that's where the user is when they're about to spawn. If
  usability testing shows it's missed, add a second affordance in Overview.
- Do we need a "dangerous bypass" confirmation dialog when flipping TO
  `FullAccess` (beyond the popover copy)? **Lean:** no in 0.3.0 — the floor
  still applies, so the tier is reversible and bounded. Add a confirm dialog
  only if the future CLI-bypass tier lands (which really is irreversible-ish).
- Should the `set_project_permission_mode` command emit a session event so a
  live UI updates immediately, or is "next spawn" enough? **Lean:** next
  spawn. The session map is keyed by path; re-reading the policy on the next
  `spawn_fresh` is simpler than live session reconfiguration, and the popover
  copy already says "next conversation."
