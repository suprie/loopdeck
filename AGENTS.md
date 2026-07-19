# LoopDeck — AI Project Manager Desktop App

## Tech Stack

- **Frontend**: Tauri v2 + Vite + React 19 + TypeScript
- **Backend**: Rust (src-tauri/)
- **Storage**: `.loopdeck/project.yaml` (per-repo) + `~/.config/loopdeck/config.yaml` (global registry)
- **Database**: None — local-first, offline-first

## Project Structure

```
loopdeck/
├── src/                         # Vite + React frontend (TypeScript)
│   ├── main.tsx                 # Entry point
│   ├── App.tsx / App.css        # Root layout
│   ├── types/index.ts           # TS types mirroring Rust structs
│   ├── lib/tauri.ts             # Typed IPC wrappers
│   ├── store/appStore.ts        # Zustand store
│   ├── hooks/                   # Async IPC hooks
│   └── components/              # UI components by domain
├── src-tauri/                   # Rust + Tauri backend
│   ├── src/
│   │   ├── main.rs              # Thin entry point
│   │   ├── lib.rs               # Tauri builder, state, command reg
│   │   ├── error.rs             # AppError enum + Serialize
│   │   ├── config.rs            # GlobalConfig, load/save
│   │   ├── scanner.rs           # Repo discovery by marker files
│   │   ├── project.rs           # .loopdeck/ bootstrap + desc gen
│   │   ├── memory.rs            # .loopdeck/ decisions & loops parser
│   │   └── commands.rs          # Tauri IPC handlers (12 commands)
│   ├── capabilities/default.json
│   └── tauri.conf.json
├── docs/
│   └── PRD.md                   # Product requirements
└── .Codex/skills/              # Dev skills for LoopDeck
```

## Key Architecture Decisions

- **State**: `Mutex<GlobalConfig>` managed by Tauri, serialized command execution
- **Scanner**: `walkdir` with marker-file detection (`.git`, `Cargo.toml`, `package.json`, etc.)
- **Config paths**: `directories` crate for XDG cross-platform resolution
- **Frontend state**: Zustand with selector subscriptions, typed IPC wrappers (never raw `invoke()`)
- **Error handling**: `thiserror` + manual `serde::Serialize` for structured IPC errors

## Development Commands

```bash
# Install dependencies
npm install

# Run in development mode (hot reload)
npm run tauri dev

# Build for production
npm run tauri build

# Rust tests
cd src-tauri && cargo test

# Rust lint
cd src-tauri && cargo clippy

# Frontend type-check
npx tsc --noEmit
```

## Skills

Use these skills when working on LoopDeck:

| Skill | When to use |
|---|---|
| `/rust-expert` | Writing Rust backend code in `src-tauri/` |
| `/rust-code-reviewer` | Reviewing Rust backend code |
| `/tauri-expert` | Tauri v2 config, IPC, capabilities, plugins |
| `/tauri-code-reviewer` | Reviewing Tauri setup, security, and integration |
| `/vite-senior-engineer` | Writing frontend code in `src/` |

## PRD Reference

See `docs/PRD.md` for full product requirements — V1 focuses on:
1. Discover local repositories
2. Create `.loopdeck/project.yaml` memory structure
3. Generate project descriptions (README.md → description)
4. Maintain local registry at `~/.config/loopdeck/config.yaml`

V1 does NOT include agents, loops, cloud, or collaboration.

## .loopdeck/ Memory Convention

This project itself is a LoopDeck-tracked project. AI agents working on this repo
MUST update the `.loopdeck/` memory files after significant work.

### After each session, update:

| File | Action |
|------|--------|
| `.loopdeck/decisions.md` | Append any architectural decisions made (date, status, context) |
| `.loopdeck/loops.md` | Update current loop status, next steps, move completed loops to history |

### Decision format

```markdown
## YYYY-MM-DD — Title
- **Status**: proposed | accepted | superseded
- **Context**: Why this was needed.
- **Consequences**: What changed.
```

### Loop format

```markdown
## Current
- **Started**: YYYY-MM-DD
- **Goal**: What we're building
- **Status**: in_progress

## Next Steps
- [ ] Pending task

## History
### YYYY-MM-DD — Completed loop title
- **Status**: completed
- **Completed**: YYYY-MM-DD
```

The Stop hook in `.Codex/settings.local.json` enforces this convention automatically.

