---
prd: prd-trust-boundary-hardening
status: proposed
priority: P0
description: >
  Harden LoopDeck's agent execution, persistence, project filesystem boundary,
  and session recovery without introducing a database, container runtime, or
  generalized workflow engine.
---

# PRD — Trust Boundary and Recovery Hardening

## Overview

LoopDeck has grown from a local project registry into an agent execution
environment. It now launches a persistent Claude process, grants tools access
to user-selected repositories, writes project and global state, streams agent
events, and parks running turns for questions or permissions.

Those capabilities are valuable, but they move four concerns into the product's
core contract:

1. The permission behavior shown to the user must match the behavior actually
   enforced by Claude Code and generated project settings.
2. Important local files must survive crashes, full disks, and malformed data.
3. A repository must be treated as untrusted input and remain inside its
   registered filesystem boundary.
4. Interrupted agent sessions must recover into a clear, truthful state.

This PRD addresses those concerns with small, centralized mechanisms. It does
not attempt to make LoopDeck a sandbox, distributed system, or workflow engine.

## Problem Statement

### Permission behavior has multiple sources of truth

The current runtime combines:

- Claude Code's `--permission-mode acceptEdits`;
- generated `.claude/settings.json` allow rules, including `Edit(*)`,
  `Write(*)`, and broad build-runner patterns;
- LoopDeck's manual-approval tool list; and
- an allow-by-default fallback with a destructive-command floor.

Calls approved by Claude Code settings never reach LoopDeck's permission
handler. This means the UI can imply that a mutation requires approval while an
allow rule or `acceptEdits` permits it without presenting an approval card.
Broad `npm`, `cargo`, and similar rules are also not inherently safe because a
repository controls its scripts and build steps.

### Persistence is not consistently crash-safe

The registry, project metadata, loop state, PRDs, and generated settings are
commonly replaced with direct file writes. A process crash or full disk can
leave a truncated file. A malformed global registry currently falls back to a
fresh default at startup, which can make recoverable corruption look like data
loss and may overwrite the evidence needed for recovery.

### Project boundaries are enforced command by command

Several commands correctly canonicalize paths and reject traversal, but there
is no single boundary helper used by every project-scoped operation. Imported
repositories may contain symlinks, scripts, hooks, very large files, and prompt
content. Correctness should not depend on each new command independently
reimplementing containment checks.

### Session recovery is mostly in-memory

Live sessions, pending questions, permissions, and interrupt slots are held in
memory. App shutdown, renderer reload, child-process exit, or removal of a
project during a turn can leave the UI without a durable explanation of what
happened. Parked turns can also hold a project lock indefinitely.

## Goals

| Priority | Goal |
|---|---|
| P0 | Make the effective agent permission policy explicit, testable, and consistent with the UI |
| P0 | Ensure mutating or executing capabilities cannot bypass LoopDeck approval through generated broad allow rules |
| P0 | Make critical YAML, Markdown, JSON, and transcript state crash-safe and recoverable |
| P1 | Centralize registered-project path validation and containment checks |
| P1 | Bound recursive scans, event accumulation, transcript lines, and parked turns |
| P1 | Recover interrupted sessions into a truthful terminal state after restart or child failure |
| P2 | Add focused cross-boundary tests and make existing build/lint checks enforceable in CI |
| P2 | Persist only navigation identifiers in the renderer, keeping Rust/on-disk state authoritative |

## Non-Goals

- OS-level sandboxing, containers, virtual machines, or per-agent users.
- A database or event-sourcing architecture.
- Cloud sync, collaboration, or multi-user permissions.
- A generalized workflow, policy, or plugin engine.
- Perfect static classification of arbitrary shell commands.
- Automatically restoring a child process that died during a turn.
- Redesigning the agent or project-management UI beyond the minimum needed to
  communicate permissions and recovery state.

## Product and Security Contract

### Trust boundaries

- The LoopDeck application and its bundled code are trusted.
- Imported repositories and everything inside them are untrusted input.
- Agent/model output is untrusted input.
- User-selected project roots are authorized for project-scoped work; paths
  outside them are not implicitly authorized.
- External commands and MCP tools may mutate local or remote state and require
  an explicit policy decision.

### Permission modes

LoopDeck exposes two understandable modes:

1. **Confirm changes** — default. Read-only local inspection is automatic.
   File mutation, command execution, network fetches, and MCP tools require an
   approval unless the user has saved a narrow matching rule.
2. **Autonomous project** — explicit per-project opt-in. File mutation inside
   the canonical project root may proceed automatically. Command execution,
   MCP calls, and operations outside the project still follow their policy.

The effective mode must be visible in the Agent UI. Changing modes is a user
action, not an import side effect.

### Rules

- Generated project settings must not install `Edit(*)`, `Write(*)`, or broad
  build-runner rules as hidden defaults.
- A remembered rule is narrow: tool plus a stable command/capability pattern.
- Destructive-floor denials are enforced before manual or remembered approval.
- A tool call must have one auditable decision path. The Claude Code mode,
  settings sources, LoopDeck policy, and UI must not contradict each other.
- LoopDeck does not claim that an allow/deny list is an OS sandbox.

## Functional Requirements

### FR1 — Effective permission policy

- Define one backend representation of the effective permission mode.
- Spawn Claude Code using a mode that routes policy-relevant requests through
  LoopDeck rather than silently approving mutations.
- Remove unsafe generated defaults while preserving user-authored settings.
- Show tool name, relevant target/command, scope, and decision on approval
  cards and in the transcript.
- Expire pending approvals/questions after a bounded duration and surface the
  expiry as an interrupted/denied outcome.
- Add tests proving `Edit`, `Write`, `Bash`, `WebFetch`, and MCP requests take
  the intended path under both supported modes.

### FR2 — Atomic persistence and recovery

- Introduce a shared atomic-write helper using a sibling temporary file,
  flush/sync as appropriate, and same-filesystem rename.
- Apply it to the global registry, `.loopdeck/project.yaml`, loop state, PRDs,
  generated Claude settings, and whole-file transcript rewrites.
- Preserve owner-only permissions for secret-adjacent global configuration.
- Keep one last-known-good backup of the global registry.
- On malformed registry data, do not overwrite the malformed file. Load the
  backup when valid or return a recoverable startup error with the affected
  paths.
- Append-only transcript writes must be line-atomic from LoopDeck's point of
  view and tolerate a partial final line during recovery.

### FR3 — Registered project boundary

- Add a shared helper that resolves a supplied project path to a canonical,
  registered project root.
- Add a shared helper that resolves relative paths beneath that root and
  rejects traversal and symlink escape.
- Route all project-scoped IPC commands through these helpers.
- Do not follow symlinked directories during recursive scanning/search.
- Import must disclose files/settings LoopDeck will create or change; it must
  not execute repository-provided hooks merely because the project was
  imported.

### FR4 — Resource limits

- Bound scan depth, visited entries, returned candidates, and wall-clock work.
- Bound README/spec/transcript/event line sizes before loading them wholly into
  memory.
- Bound `ResponseAccumulator` blocks and bytes.
- Put synchronous filesystem traversal and subprocess metadata work on the
  blocking pool.
- Return structured limit errors that leave the app usable.

### FR5 — Session lifecycle and recovery

- Assign every turn a run ID and track: project, state, start time, last event
  time, and terminal reason.
- Persist only the small run record needed to explain recovery; conversation
  content remains in the existing transcript format.
- On startup, mark non-terminal prior runs as `interrupted` rather than
  pretending they are still working or attempting transparent resurrection.
- Handle child exit, interrupt, approval expiry, renderer reload, and project
  removal without leaving a permanent busy/waiting state.
- Removing a project with a live run must require stopping that run first.

### FR6 — Renderer authority

- Persist only `selectedProjectPath`, selected tab, theme, and similar harmless
  preferences in browser storage.
- Reload project data and run state from Rust after startup/reload.
- Unknown backend event variants fail visibly and safely rather than being
  silently ignored.

## Phases

### Phase 1 — Permission contract (P0)

- [ ] Document the effective tool-decision flow from Claude settings to LoopDeck response
- [ ] Add backend `ConfirmChanges` and `AutonomousProject` permission modes with `ConfirmChanges` as default
- [ ] Remove generated `Edit(*)`, `Write(*)`, and broad build-runner allow rules
- [ ] Reconcile Claude's `--permission-mode` and settings sources with the backend policy
- [ ] Display the effective mode and approval scope in the Agent UI
- [ ] Add permission-path regression tests for edits, writes, Bash, network, and MCP tools

### Phase 2 — Crash-safe persistence (P0)

- [ ] Implement and test a shared atomic-write helper
- [ ] Migrate the global registry and preserve a last-known-good backup
- [ ] Stop replacing malformed config with a freshly persisted default
- [ ] Migrate project metadata, loops, PRDs, and generated settings to atomic writes
- [ ] Verify transcript recovery ignores a partial final line without losing earlier turns

### Phase 3 — Project and resource boundaries (P1)

- [ ] Centralize canonical registered-project validation
- [ ] Centralize relative-path containment and symlink-escape rejection
- [ ] Apply the helpers to every project-scoped IPC command
- [ ] Add scan/search depth, entry, result, byte, and time budgets
- [ ] Cap agent response accumulation and input/event line sizes

### Phase 4 — Session recovery (P1)

- [ ] Add bounded expiry for parked questions and permission slots
- [ ] Persist a minimal run record with run ID, state, timestamps, and terminal reason
- [ ] Reconcile stale runs to `interrupted` on startup
- [ ] Handle child exit, renderer reload, interrupt, and project removal deterministically
- [ ] Add integration tests for the session failure matrix

### Phase 5 — Authority and quality gates (P2)

- [ ] Persist only the selected project path and UI preferences in Zustand
- [ ] Add boundary tests for import/restart, corrupt-config recovery, path escape, approval routing, and interruption
- [ ] Make frontend build, Rust tests, formatting, and Clippy required CI checks
- [ ] Resolve the existing Clippy failures before enabling `-D warnings`

## Acceptance Criteria

- A fresh import cannot silently install broad edit/write/runner permissions.
- In `ConfirmChanges`, representative mutating and executing tools always
  produce a LoopDeck decision or match a user-created narrow rule.
- The UI accurately displays the effective policy for the current project.
- Killing LoopDeck during replacement of any critical file leaves either the
  old complete version or the new complete version, never a truncated primary.
- A malformed primary registry is preserved and a valid backup is recoverable.
- Project-relative path and symlink escape tests fail closed across all
  project-scoped commands.
- Large repositories and oversized agent events return bounded errors without
  freezing or exhausting the application.
- Restarting after an in-flight turn shows it as interrupted and permits a new
  turn without manual state cleanup.
- `npm run build`, `cargo test`, and the enforced Clippy/format CI checks pass.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| More approval prompts reduce autonomy | Explicit per-project autonomous mode and narrow remembered rules |
| Claude Code mode/settings semantics change | Contract tests around representative control requests; keep integration isolated in the session adapter |
| Atomic rename differs across platforms | Create temp files beside targets and test on each supported CI OS |
| Central path validation breaks moved projects | Return a structured missing/moved state and support explicit re-linking later |
| Limits reject a legitimate large monorepo | Start with generous constants, return the breached limit, and make only existing scan depth configurable |
| Minimal run records drift from transcripts | Run state explains lifecycle only; transcript remains the content source of truth |

## Verification Strategy

- Unit-test permission decisions, path resolution, atomic writes, recovery, and
  size/depth limits without a live provider.
- Add adapter-level tests with a fake child process/control stream for approval,
  expiry, interrupt, malformed event, and child-exit behavior.
- Keep live provider and OS-keychain tests ignored/manual, as they are today.
- Run the existing production build and 257-test Rust suite throughout the
  work; do not defer regression cleanup until the final phase.

## Implementation Order

The phases are intentionally ordered by blast radius: make permissions honest
first, protect durable state second, centralize boundaries third, then improve
session recovery and enforcement. Each phase is independently shippable and
must leave the application usable.

