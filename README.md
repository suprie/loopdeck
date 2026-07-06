# LoopDeck

**Local-first desktop app for structured project memory — stored right inside your repo.**

LoopDeck discovers your local repositories and creates a standardized `.loopdeck/` folder in each one, carrying project descriptions, architectural decisions, and development loops alongside your source code. No cloud, no database, no lock-in.

## Why?

Developers working with AI tools lose project context constantly — purpose, decisions, constraints, current goals — all scattered across chat histories and notes. When you switch projects, you re-explain everything from scratch.

LoopDeck solves this by storing project memory **inside the repository** using a simple, file-based convention:

```
your-repo/
└── .loopdeck/
    ├── project.yaml      # name, description, status, timestamps
    ├── decisions.md      # architectural decision log
    └── loops.md          # current work loop and history
```

Any tool — AI agent, CLI, editor — can read and write these files. LoopDeck provides the desktop UI.

## What V1 Does

- 🔍 **Scans** a directory recursively for repositories (`.git`, `Cargo.toml`, `package.json`, `go.mod`, and more)
- 📦 **Imports** selected repos with one click
- 📝 **Bootstraps** `.loopdeck/project.yaml` with a generated description (parsed from `README.md`)
- 🗂️ **Maintains** a local registry at `~/.config/loopdeck/config.yaml`
- ✏️ **Edits** descriptions inline, or regenerates from README
- 📊 **Shows** git freshness (last commit, uncommitted changes) and relative timestamps
- 🔗 **Opens** projects in Finder or Terminal directly from the UI

## Quick Start

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (stable)

### Install & Run

```bash
# Install frontend dependencies
npm install

# Launch in development mode (hot reload)
npm run tauri dev

# Build for production
npm run tauri build
```

### Verify

```bash
# Run Rust tests (30 tests, 4 modules)
cd src-tauri && cargo test

# Rust lint
cargo clippy

# Frontend type-check
npx tsc --noEmit
```

## Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri v2 |
| Frontend | Vite + React 19 + TypeScript |
| Backend | Rust |
| State | Zustand |
| Icons | Lucide React |
| Styling | Plain CSS (dark theme) |
| Storage | YAML files on disk — no database |

### Architecture

```
┌─────────────────────────────────┐
│  React 19 + Vite + TypeScript   │
│  ┌──────────┐  ┌─────────────┐  │
│  │ Zustand  │  │ Typed IPC   │  │
│  │ Store    │  │ Wrappers    │  │
│  └──────────┘  └──────┬──────┘  │
├───────────────────────┼─────────┤
│  Tauri v2 IPC Bridge  │         │
├───────────────────────┼─────────┤
│  Rust Backend         ▼         │
│  ┌──────────────────────────┐   │
│  │ Mutex<GlobalConfig>      │   │
│  │ scanner │ project │ git  │   │
│  │ config  │ memory  │ cmds │   │
│  └──────────────────────────┘   │
├─────────────────────────────────┤
│  File System                    │
│  .loopdeck/  +  ~/.config/      │
└─────────────────────────────────┘
```

## Project Structure

```
loopdeck/
├── src/                    # React frontend (TypeScript)
│   ├── components/         # UI by domain: dashboard, detail, import, layout, shared
│   ├── store/              # Zustand state management
│   ├── hooks/              # Async IPC hooks
│   ├── lib/                # Typed Tauri IPC wrappers, time utilities
│   └── types/              # TS interfaces mirroring Rust structs
├── src-tauri/              # Rust backend
│   └── src/
│       ├── commands.rs     # 10 Tauri IPC handlers
│       ├── config.rs       # GlobalConfig, XDG path resolution
│       ├── scanner.rs      # Repo discovery (walkdir + marker files)
│       ├── project.rs      # .loopdeck/ bootstrap, README parsing
│       ├── memory.rs       # decisions.md / loops.md parser
│       ├── git.rs          # Git freshness detection
│       └── error.rs        # AppError (thiserror + serde)
├── docs/PRD.md             # Full product requirements
└── .loopdeck/              # LoopDeck's own project memory (dogfooding)
```

## IPC Commands

| Command | What it does |
|---------|-------------|
| `scan_directory` | Recursively discover repos by marker files |
| `import_project` | Bootstrap `.loopdeck/` and register project |
| `list_projects` | List all registered projects |
| `get_project` | Get a single project by path |
| `update_description` | Edit a project's description |
| `regenerate_description` | Re-parse README.md for description |
| `remove_project` | Remove from registry (keeps files) |
| `rescan_project` | Refresh git info for a project |
| `open_in_finder` | Reveal in system file manager |
| `open_in_terminal` | Open in system terminal |

All commands are typed on the frontend — raw `invoke()` is never called from components.

## V1 Scope

✅ Discover local repositories  
✅ Create `.loopdeck/project.yaml` memory structure  
✅ Generate project descriptions from README.md  
✅ Maintain local registry at `~/.config/loopdeck/config.yaml`  
✅ 30 Rust tests passing, full frontend type-check  

V1 does **not** include agents, loops, cloud, or collaboration — those are V2 candidates.

## What's Next

- **Memory expansion**: `decisions.md`, `loops.md`, `activity.md`, `agents.md`
- **Claude Code integration**: detect `.claude/`, sync context
- **Agent execution**: run AI agents against project memory
- **Activity tracking**: open/modify timestamps, session history

## License

MIT
