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
│   ├── App.tsx / router.tsx     # Root layout + TanStack Router routes
│   ├── styles.css               # Design tokens (dark theme), shared button styles
│   ├── types/index.ts           # TS types mirroring Rust structs
│   ├── lib/                     # 7 files: tauri.ts (typed IPC wrappers), utils, time, markdown, theme, attachments, agent roster client
│   ├── store/                   # 3 Zustand stores: appStore, pendingInteractions, streamingState
│   ├── hooks/                   # 4 async IPC hooks: projects, activity, run queue, stuck sessions
│   └── components/              # 68 components by domain (13 dirs: dashboard, import, detail, agent, activity, decisions, loops, epics, settings, spec, shared, ui, layout)
├── src-tauri/                   # Rust + Tauri backend
│   ├── src/                     # 32 top-level modules
│   │   ├── main.rs              # Thin entry point
│   │   ├── lib.rs               # Tauri builder, state, 81 command registration
│   │   ├── error.rs             # AppError enum + Serialize
│   │   ├── config.rs            # GlobalConfig, AgentConfig, load/save
│   │   ├── scanner.rs           # Repo discovery by marker files
│   │   ├── project.rs           # .loopdeck/ bootstrap + desc gen
│   │   ├── memory.rs            # .loopdeck/ decisions & loops parser
│   │   ├── execution.rs         # .loopdeck/execution.yaml loop state + transitions
│   │   ├── epic.rs              # Epic → PRD → Phase → Loop spec layer
│   │   ├── agents.rs            # Agent runtime, config resolution
│   │   ├── claude_session.rs    # Claude Code session adapter
│   │   ├── codex_session.rs     # Codex CLI adapter
│   │   ├── harness.rs           # Agent harness dispatch
│   │   ├── multi_agent.rs       # Multi-agent concurrent runs
│   │   ├── runplan.rs           # Run plan data model + persistence
│   │   ├── run_executor.rs      # Unattended run execution + budgets
│   │   ├── secret_scan.rs       # Staged-diff credential scan
│   │   ├── skills.rs            # Skill discovery/indexing
│   │   └── commands/            # 81 Tauri IPC handlers (9 files: project, agent, config_cmds, epics, execution, run_queue, composer, state)
│   ├── capabilities/default.json
│   └── tauri.conf.json
├── docs/
│   ├── PRD.md                   # Product requirements (V1 historical + amendments)
│   └── epics/                   # Epic → PRD spec layer
└── .agents/skills/              # Dev skills for LoopDeck (loopdeck-*)
```

## Key Architecture Decisions

- **State**: `AppState` (`commands/state.rs:37`) holding `Mutex<GlobalConfig>` config plus per-project agent-session, pending-question/permission/plan, and run-handle maps, managed by Tauri
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
| `loopdeck-orchestrator` | Build from a PRD: clarify, plan, build, review, verify, ship |
| `loopdeck-loop-runner` | Execute a single queued loop end-to-end |
| `loopdeck-prd-verifier` | Verify implemented code against a PRD's acceptance criteria |
| `loopdeck-open-pr` | Ship the current branch as a reviewable (draft) PR |
| `loopdeck-memory` | `.loopdeck/` write conventions (decisions.md, loops.md) |
| `loopdeck-epic-author` | Author epics / PRDs under `docs/epics/` |
| `loopdeck-vite-senior-engineer` | Writing frontend code in `src/` |

## PRD Reference

See `docs/PRD.md` for the V1 product requirements (historical) and its
Amendments section for the current scope. The app has grown beyond V1: it runs
AI agents (Claude Code + Codex), executes engineering loops from a run queue,
and tracks epics/PRDs/decisions. Cloud sync and team collaboration remain out
of scope.

## Context Discipline (token cost)

This repo has several large files. Re-reading them across iterations is the
dominant token cost in long sessions. Current large files (line counts):

- `claude_session.rs` ~2,844 lines
- `epic.rs` ~2,449 lines
- `conversation.rs` ~2,227 lines
- `commands/agent.rs` ~2,221 lines
- `commands/run_queue.rs` ~1,991 lines
- `agents.rs` ~1,905 lines
- `config.rs` ~1,480 lines

- **Do not re-read a file you have already read this session.** The contents are
  in your context already; use `Grep`/offset-`Read` to locate a specific symbol
  instead of re-reading the whole file.
- **Prefer `Grep` over full `Read`** when you need one function or constant.
- **State which file/line you're reasoning from** so the next turn doesn't need
  to re-read to verify.
- If a file has grown past ~1500 lines, flag it for splitting in `loops.md`.

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

The Stop hook (`templates/hooks/loopdeck-stop-hook.py`) enforces this convention automatically.

