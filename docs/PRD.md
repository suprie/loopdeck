# PRD - LoopDeck V1

## Status: ✅ Implemented (2026-06-22)

All four V1 goals are complete. The application builds, 30 Rust tests pass, and the full TypeScript frontend type-checks cleanly.

---

## Overview

LoopDeck is a local-first desktop application that helps developers maintain structured project memory alongside their source code repositories.

Instead of storing project knowledge inside a proprietary database, LoopDeck stores all project context directly inside the repository using a standardized `.loopdeck` folder.

The application acts as a repository scanner and project memory manager.

Future versions may add AI agents, engineering loops, and automation. Version 1 focuses exclusively on project discovery and memory initialization.

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

---

## Non Goals

Version 1 will NOT include:

- Agent execution
- Claude Code integration
- Codex integration
- Prompt generation
- Next Loop suggestions
- Activity tracking
- Decision tracking
- Cloud synchronization
- Team collaboration

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

✅ Implemented in `commands.rs::import_project` — checks for existing entry, creates `.loopdeck/` directory, bootstraps `project.yaml`, registers in global config.

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
    └── project.yaml
```

Only a single file is created in Version 1.

Future versions may add:

- `decisions.md`
- `loops.md`
- `activity.md`
- `agents.md`
- `context.md`

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

Implemented in `Dashboard.tsx`, `ProjectRow.tsx`, `EmptyState.tsx`.

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

| Command | Signature | Description |
|---------|-----------|-------------|
| `scan_directory` | `(path: String) -> Vec<DiscoveredRepo>` | Recursive repo discovery |
| `import_project` | `(path: String) -> ProjectEntry` | Bootstrap `.loopdeck/` + register |
| `list_projects` | `() -> Vec<ProjectEntry>` | List all registered projects |
| `get_project` | `(path: String) -> ProjectEntry` | Get single project by path |
| `update_description` | `(path: String, desc: String) -> ProjectMeta` | Edit project description |
| `remove_project` | `(path: String) -> ()` | Remove from registry (keeps files) |
| `rescan_project` | `(path: String) -> ProjectEntry` | Refresh git info for a project |
| `regenerate_description` | `(path: String) -> String` | Re-generate desc from README |
| `open_in_finder` | `(path: String) -> ()` | Open in system file manager |
| `open_in_terminal` | `(path: String) -> ()` | Open in system terminal |

All commands are typed on the frontend via `src/lib/tauri.ts` wrappers — raw `invoke()` is never called from components.

---

## Tech Stack (Actual)

| Layer | Technology |
|---|---|
| Frontend | Tauri v2 + Vite + React 19 + TypeScript |
| Backend | Rust (src-tauri/) |
| State Management | Zustand (selector-based subscriptions) |
| IPC | Tauri v2 commands via typed wrappers |
| Storage (Repo) | `.loopdeck/project.yaml` |
| Storage (Global) | `~/.config/loopdeck/config.yaml` |
| Database | None — local-first, offline-first |
| Icons | Lucide React |
| Styling | Plain CSS with design tokens (dark theme) |

---

## Source File Map

```
src-tauri/src/
├── main.rs             # Thin entry point
├── lib.rs              # Tauri builder, state, 10 command registration
├── error.rs            # AppError enum + Serialize (thiserror + serde)
├── config.rs           # GlobalConfig, ProjectEntry, Settings, XDG paths
├── scanner.rs          # Repo discovery (walkdir + markers), depth limit
├── project.rs          # .loopdeck/ bootstrap, README parsing, desc gen
├── git.rs              # Git freshness: last commit, dirty, last modified
└── commands.rs         # 10 Tauri IPC handlers

src/
├── main.tsx            # React entry point
├── App.tsx             # View routing (dashboard | import | detail)
├── App.css             # Design tokens (dark theme), shared button styles
├── types/index.ts      # TS types mirroring Rust structs
├── lib/tauri.ts        # Typed IPC wrappers (never raw invoke())
├── lib/time.ts         # Relative time formatting
├── store/appStore.ts   # Zustand store with actions
├── hooks/useProjects.ts # Async IPC hook
└── components/
    ├── layout/AppShell.tsx       # Header, error bar, Scan Folder button
    ├── dashboard/Dashboard.tsx   # Project list view
    ├── dashboard/EmptyState.tsx  # First-launch empty state
    ├── dashboard/ProjectRow.tsx  # Single project row with actions
    ├── import/ImportFlow.tsx     # Import flow with discovered repo list
    ├── import/RepoCard.tsx       # Repo card with stack, preview, import
    ├── detail/ProjectDetail.tsx  # Project detail with metadata
    ├── detail/EditDescription.tsx # Inline description editing
    ├── shared/ConfirmDialog.tsx  # Reusable confirmation dialog
    └── shared/LoadingSpinner.tsx # Reusable loading spinner
```

---

## Test Coverage

30 Rust unit tests across 4 test modules:

| Module | Tests | Coverage |
|--------|-------|----------|
| `scanner` | 11 | Marker detection, depth enforcement, child skipping, ignore dirs, stack detection |
| `project` | 10 | Bootstrap, load, update, README extraction (headings, badges, code blocks), fallbacks |
| `config` | 6 | Add/remove/find, roundtrip serialization, empty config, file persistence |
| `git` | 3 | No-git directory, committed repo, uncommitted changes |

All tests: `cargo test` — 30 passed, 0 failed.

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
| Dashboard didn't show "Last Opened" | `ProjectRow` now displays relative "Opened" time |
| Import view didn't show detected stack or description preview | `DiscoveredRepo` now carries `detected_stack` + `description_preview`; `RepoCard` renders both |
| No "Rescan" action on Dashboard | `rescan_project` command + per-project Refresh button refreshes git info |

---

## What's Next (V2 Candidates)

From the deferred features list:

- **Project memory expansion**: `decisions.md`, `loops.md`, `activity.md`, `agents.md`, `context.md`
- **Claude Code integration**: Detect `.claude/` folders, read/write CLAUDE.md, sync context
- **Agent execution**: Run AI agents against project context
- **Activity tracking**: Track when projects are opened, modified, etc.
- **Decision tracking**: Structured architectural decision records
