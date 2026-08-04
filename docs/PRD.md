# PRD - LoopDeck V1 (Historical)

## Status: ✅ Implemented (2026-06-22) — superseded

V1 shipped 2026-06-22: repository discovery, `.loopdeck/` project memory, and a
local registry. The app has since grown well beyond V1 scope — it runs AI
agents (Claude Code and Codex), executes engineering loops from a run queue,
and tracks epics, PRDs, and decisions. The V1 body below is retained as the
historical record of what V1 built; the **Amendments** section records what
shipped after and what the current scope is.

Current state (2026-08-04):

- **Backend**: `src-tauri/src/` — 32 top-level `.rs` files plus a `commands/`
  directory (9 files) holding 81 Tauri IPC handlers.
- **Tests**: 594 test functions across 34 modules (586 pass, 8 ignored).
- **Runtime features** beyond V1: agent execution (`agents.rs`,
  `claude_session.rs`, `codex_session.rs`, `harness.rs`), engineering loops and
  a run queue (`execution.rs`, `runplan.rs`, `run_queue.rs`), and the
  Epic → PRD → Phase → Loop spec layer (`epic.rs`).

---

## Amendments

### 2026-08-04 — Docs-accuracy reconciliation (`prd-docs-accuracy`)

This PRD froze at V1 scope on 2026-06-22. Code shipped afterward violated
several non-goals the V1 text stated as current constraints. Each shipped
non-goal below is marked historical with a pointer to the epic that shipped
it; the ones that never shipped remain excluded.

| Non-goal (as written in V1) | Status | Shipped by |
|---|---|---|
| Agent execution | **Shipped** | `agent-full-access` (autonomous agent runtime, verify/ship) |
| Claude Code integration | **Shipped** | `multi-model-agents` (`claude_session.rs`) |
| Codex integration | **Shipped** | `multi-model-agents` (`codex_session.rs`) |
| Prompt generation | Still excluded | — |
| Next Loop suggestions | **Shipped** | `overnight-orchestration` (run queue, `promote_next_queued_loop`) |
| Activity tracking | **Shipped** | `support-project-management` (activity/decisions/loops views) |
| Decision tracking | **Shipped** | `support-project-management` (decisions parser + view) |
| Cloud synchronization | Still excluded | — |
| Team collaboration | Still excluded | — |

The V1 body below is the historical record. Where it contradicts the current
tree, a `> **V1 note:**` callout marks the superseded claim.

---

## Overview

LoopDeck is a local-first desktop application that helps developers maintain structured project memory alongside their source code repositories.

Instead of storing project knowledge inside a proprietary database, LoopDeck stores all project context directly inside the repository using a standardized `.loopdeck` folder.

The application acts as a repository scanner and project memory manager.

> **V1 note:** V1 focused "exclusively on project discovery and memory
> initialization"; the shipped app extends into AI agent execution and
> engineering loops (see Amendments).

---

## Problem Statement

Developers working with AI tools frequently lose project context.

Important information such as:

- project purpose
- architecture decisions
- current goals
- technical constraints

is scattered across chat histories, notes, and prompts.

When switching between projects, developers repeatedly explain the same context to AI assistants.

There is no standard, repository-native way to persist project memory.

---

## Goals

Version 1 goals:

1. ✅ Discover local repositories.
2. ✅ Create a standardized project memory structure.
3. ✅ Generate a brief project description.
4. ✅ Maintain a local registry of tracked projects.

All four shipped in V1. Follow-on epics extended the product beyond these
goals; see the Amendments table and `docs/epics/` for the record.

---

## Non Goals (historical)

Version 1 (2026-06-22) did not include the items below. Six of the nine have
since shipped; see the Amendments table for each item's status and the epic
that shipped it. The remaining three — prompt generation, cloud
synchronization, and team collaboration — are still excluded.

- ~~Agent execution~~ → shipped (`agent-full-access`)
- ~~Claude Code integration~~ → shipped (`multi-model-agents`)
- ~~Codex integration~~ → shipped (`multi-model-agents`)
- Prompt generation — still excluded
- ~~Next Loop suggestions~~ → shipped (`overnight-orchestration`)
- ~~Activity tracking~~ → shipped (`support-project-management`)
- ~~Decision tracking~~ → shipped (`support-project-management`)
- Cloud synchronization — still excluded
- Team collaboration — still excluded

---

## User Flow

### First Launch

User opens LoopDeck.

Application displays:

> "No projects found."

User clicks:

> Scan Folder

✅ Implemented in `EmptyState.tsx` — shows icon, message, and Scan Folder button.

---

### Scan Folder

User selects a directory.

LoopDeck recursively searches for:

- `.git`
- `Package.swift`
- `Cargo.toml`
- `go.mod`
- `package.json`
- `Gemfile`
- `Podfile`
- `*.xcodeproj`
- `*.xcworkspace`

Potential repositories are displayed with:
- Repository name
- Detected technology stack (e.g. "Rust, JavaScript/TypeScript")
- Description preview (generated from detected stack)
- Git freshness (last commit, last modified)

Scan depth is configurable via `settings.scan_depth` (default: 5).

✅ Implemented in `scanner.rs` (Rust) + `ImportFlow.tsx` / `RepoCard.tsx` (React).

---

### Import Repository

User selects a repository.

LoopDeck checks:

> `repo/.loopdeck`

If folder exists:

> Load project.

If folder does not exist:

> Create project memory structure.

✅ Implemented in `commands/project.rs::import_project` — checks for existing entry, creates `.loopdeck/` directory, bootstraps `project.yaml`, registers in global config.

---

### Bootstrap Project

LoopDeck generates:

> `repo/.loopdeck/project.yaml`

Example:

```yaml
name: Budget Manager

description: |
  Local-first budgeting application for importing
  and categorizing Indonesian bank statements.

status: active

created_at: 2026-06-22
```

LoopDeck then registers the project.

✅ Implemented in `project.rs` — `bootstrap_project()` creates `.loopdeck/`, generates description from README or stack, writes `project.yaml`.

---

## File Structure

### Repository Structure

```
repo/
└── .loopdeck/
    ├── project.yaml
    ├── decisions.md
    ├── loops.md
    ├── execution.yaml
    ├── hooks/
    └── sessions/
```

> **V1 note:** V1 created only `project.yaml`; the decisions/loops memory files
> and structured execution state shipped in later epics.

---

## Global Configuration

LoopDeck stores application configuration in:

> `~/.config/loopdeck/config.yaml`

Example:

```yaml
projects:
  - path: /Users/suprie/projects/ngopi-yuk
    name: Ngopi Yuk
    description: A Rust and Tauri application.
    status: active
    last_opened: 2026-06-22T10:30:00Z
    created_at: 2026-06-22T10:00:00Z
    last_commit: "2026-06-22T09:45:00+07:00"
    last_modified: "2026-06-22T10:15:00Z"

settings:
  scan_depth: 5
```

✅ Implemented in `config.rs` — `GlobalConfig` with `ProjectEntry` and `Settings`. XDG-compliant path resolution via `directories` crate with fallback to `~/.config/loopdeck/`.

---

## Project Description Generation

When importing a repository, LoopDeck attempts to generate a short project description.

Sources:

1. README.md (first meaningful paragraph)
2. Detected technology stack
3. Repository name + markers

If README exists:

> Extract first non-heading, non-codeblock, non-badge paragraph.

If README does not exist:

> Generate a description from detected stack. Example: "A Rust project."

User can edit description manually or regenerate from README.

✅ Implemented in `project.rs::generate_description()` — README paragraph extraction with badge/codeblock/heading skipping, stack-based fallback. User can edit via `EditDescription.tsx` or regenerate via `RefreshCw` button.

---

## Main Screens

### Dashboard

Displays:

- ✅ Project Name
- ✅ Description
- ✅ Last Commit (relative time)
- ✅ Last Opened (relative time)
- ✅ Status

Actions:

- ✅ View Details (Info button)
- ✅ Rescan (RefreshCw button — updates git freshness)
- ✅ Open in Finder
- ✅ Open in Terminal
- ✅ Remove from Registry

Implemented in `Dashboard.tsx`, `ProjectList.tsx`, `EmptyState.tsx`.

---

### Import Repository

Displays:

- ✅ Repository name
- ✅ Detected technology stack (e.g. "Rust, JavaScript/TypeScript")
- ✅ Description preview (e.g. "A Rust project.")
- ✅ Git freshness
- ✅ README badge
- ✅ Already-imported badge

Actions:

- ✅ Import
- ✅ Scan Again
- ✅ Back to Dashboard

Implemented in `ImportFlow.tsx`, `RepoCard.tsx`.

---

### Project Details

Displays:

- ✅ Name
- ✅ Description (with inline edit)
- ✅ Repository Path
- ✅ Status
- ✅ Created date
- ✅ Last opened date
- ✅ Git freshness (last commit, last modified)

Actions:

- ✅ Edit Description (inline textarea)
- ✅ Regenerate Description (from README)
- ✅ Open in Finder
- ✅ Open in Terminal
- ✅ Remove from Registry (with confirmation dialog)

Implemented in `ProjectDetail.tsx`, `EditDescription.tsx`, `ConfirmDialog.tsx`.

---

## Tauri IPC Commands (API Surface)

The V1 surface below shipped the original 10 commands; the current surface is
**81 commands** registered in `src-tauri/src/lib.rs:157`, organized by domain
under `src-tauri/src/commands/`:

| Module (`commands/`) | Commands | Covers |
|---|---|---|
| `project.rs` | 12 | import/list/get/update/remove projects, open in Finder/Terminal, regenerate description, rescan, refresh skills, graphify stats |
| `agent.rs` | 22 | agent loop start/stream, send message, conversation CRUD, answer question/permission/plan, interrupt, pending-state queries |
| `config_cmds.rs` | 11 | agent config CRUD, default agent config, auth token, log info |
| `epics.rs` | 10 | decisions/loops/epics read, promote loop, toggle loop/PRD steps, assign loop ID, spec read/write |
| `execution.rs` | 9 | execution state, promote/complete/abandon loops, migration, progress |
| `run_queue.rs` | 9 | run plan create/queue/cancel, run status/report, parked-question answer, interview |
| `composer.rs` | 4 | list dir entries, search files, list skills, scan directory |
| `multi_agent.rs` (top-level) | 4 | multi-agent run start/list/control |

The original 10 commands (`scan_directory`, `import_project`, `list_projects`,
`get_project`, `update_description`, `remove_project`, `rescan_project`,
`regenerate_description`, `open_in_finder`, `open_in_terminal`) all remain
registered.

All commands are typed on the frontend via `src/lib/tauri.ts` wrappers — raw `invoke()` is never called from components.

---

## Tech Stack (Actual)

| Layer | Technology |
|---|---|
| Frontend | Tauri v2 + Vite + React 19 + TypeScript |
| Backend | Rust (src-tauri/) |
| State Management | Zustand (selector-based subscriptions) |
| IPC | Tauri v2 commands via typed wrappers |
| Storage (Repo) | `.loopdeck/project.yaml` + `.loopdeck/execution.yaml` |
| Storage (Global) | `~/.config/loopdeck/config.yaml` |
| Database | None — local-first, offline-first |
| Icons | Lucide React |
| Styling | Plain CSS with design tokens (dark theme) |

---

## Source File Map

```
src-tauri/src/                  # 32 top-level modules
├── main.rs                     # Thin entry point
├── lib.rs                      # Tauri builder, state, 81 command registration (:157)
├── error.rs                    # AppError enum + Serialize (thiserror + serde)
├── config.rs                   # GlobalConfig, ProjectEntry, Settings, AgentConfig, XDG paths
├── scanner.rs                  # Repo discovery (walkdir + markers), depth limit
├── project.rs                  # .loopdeck/ bootstrap, README parsing, desc gen
├── git.rs                      # Git freshness: last commit, dirty, last modified
├── memory.rs                   # .loopdeck/ decisions & loops parser
├── agents.rs                   # Agent runtime, config resolution
├── claude_session.rs           # Claude Code session adapter (stream-json stdin)
├── codex_session.rs            # Codex CLI adapter
├── harness.rs                  # Agent harness dispatch by provider
├── multi_agent.rs              # Multi-agent concurrent run orchestration
├── execution.rs                # .loopdeck/execution.yaml loop state + transitions
├── epic.rs                     # Epic/PRD/Phase/Loop spec layer
├── runplan.rs                  # Run plan data model + persistence
├── run_executor.rs             # Unattended run execution + budgets
├── secret_scan.rs              # Staged-diff credential scan
├── skills.rs                   # Skill discovery/indexing
├── ...                         # (paths, persist, permission, progress, retry,
│                               #  logging, limits, migration, graphify, binary,
│                               #  state_cli, secrets)
└── commands/                   # 81 Tauri IPC handlers (9 files)
    ├── mod.rs
    ├── project.rs              # 12 commands
    ├── agent.rs                # 22 commands
    ├── config_cmds.rs          # 11 commands
    ├── epics.rs                # 10 commands
    ├── execution.rs            # 9 commands
    ├── run_queue.rs            # 9 commands
    ├── composer.rs             # 4 commands
    └── state.rs                # shared helpers

src/
├── main.tsx                    # React entry point
├── App.tsx                     # Root layout
├── router.tsx                  # TanStack Router routes (dashboard | import | detail | activity | decisions | loops | epics | settings | spec)
├── styles.css                  # Design tokens (dark theme), shared button styles
├── types/index.ts              # TS types mirroring Rust structs
├── lib/                        # 7 files: tauri.ts (typed IPC wrappers), time.ts,
│                               #   utils.ts, markdown.ts, theme.tsx, attachments.ts,
│                               #   agentRosterClient.ts
├── store/                      # 3 files: appStore.ts, pendingInteractions.ts, streamingState.ts
├── hooks/                      # 4 files: useProjects.ts, useActivityEvents.tsx,
│                               #   useRunQueueEvents.ts, useStuckSessions.ts
└── components/                 # 68 components in 13 domain dirs (dashboard, import,
                                #   detail, layout, shared, ui, agent, activity,
                                #   decisions, loops, epics, settings, spec)
```

---

## Test Coverage

594 test functions across 34 modules (`src-tauri/src/` + `commands/`), of
which 586 pass and 8 are ignored. Representative module table:

| Module | Tests | Coverage |
|--------|-------|----------|
| `epic` | 56 | Spec-layer parse/build/checklist logic |
| `conversation` | 46 | Turn persistence, excerpts, listing |
| `config` | 39 | Add/remove/find, roundtrip serialization, AgentConfig |
| `permission` | 38 | Permission policy, autonomous mode |
| `agents` | 38 | Agent runtime, config resolution, env assembly |
| `run_executor` | 33 | Run execution, budgets, stalls |
| `memory` | 26 | decisions.md / loops.md parsing |
| `skills` | 26 | Skill discovery/indexing |
| `commands/run_queue` | 22 | Run queue transitions |
| `migration` | 17 | legacy loops.md → execution.yaml |
| `codex_session` | 16 | Codex adapter |
| `scanner` | 15 | Marker detection, depth, ignore dirs |
| `paths` | 15 | XDG path resolution |
| `graphify` | 15 | Graphify integration |
| `git` | 12 | No-git dir, committed repo, uncommitted changes |
| `project` | 11 | Bootstrap, load, update, README extraction |
| `commands/composer` | 11 | Dir listing, file search |
| ... | — | remaining modules + command handlers |

All tests: `cargo test` — 586 passed, 0 failed, 8 ignored.

---

## Success Metrics

- ✅ User imports first repository within 60 seconds (one-click scan + import).
- ✅ User can initialize project memory with one click.
- ✅ 100% of imported repositories contain a valid `.loopdeck/project.yaml`.
- ✅ No external services required.
- ✅ Works fully offline.

---

## V1 Gaps Resolved (2026-06-22)

| Gap | Resolution |
|-----|------------|
| `scan_depth` setting was parsed but ignored | `WalkDir::max_depth()` now enforces the limit |
| Dashboard didn't show "Last Opened" | `ProjectList` now displays relative "Opened" time |
| Import view didn't show detected stack or description preview | `DiscoveredRepo` now carries `detected_stack` + `description_preview`; `RepoCard` renders both |
| No "Rescan" action on Dashboard | `rescan_project` command + per-project Refresh button refreshes git info |

---

## What's Next (historical — V2 candidates, mostly shipped)

The V1 deferred-features list. Most shipped after V1; see the Amendments table
for the epic that delivered each.

- ~~**Project memory expansion**~~: `decisions.md` and `loops.md` parsers and
  the structured `execution.yaml` state — shipped (`memory.rs`, `execution.rs`,
  `support-project-management`); `activity.md` / `agents.md` / `context.md` were
  never created as separate files
- ~~**Claude Code integration**~~: a Claude Code session adapter that runs
  Claude CLI sessions in the agent runtime — shipped (`claude_session.rs`,
  `multi-model-agents`)
- ~~**Agent execution**~~: run AI agents against project context — shipped
  (`agents.rs`, `agent-full-access`)
- ~~**Activity tracking**~~: track when projects are opened, modified — shipped
  (`support-project-management`)
- ~~**Decision tracking**~~: structured architectural decision records —
  shipped (`support-project-management`)
