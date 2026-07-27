# LoopDeck

[![CI](https://github.com/suprie/loopdeck/actions/workflows/ci.yml/badge.svg)](https://github.com/suprie/loopdeck/actions/workflows/ci.yml)

**Local-first desktop app for structured project memory — stored right inside your repo — with a built-in agent loop that reads and writes that memory.**

LoopDeck discovers your local repositories and creates a standardized `.loopdeck/` folder in each one, carrying project descriptions, architectural decisions, and development loops alongside your source code. It also runs an on-device AI agent (the `claude` CLI) that drives the next development loop against that memory — reading `.loopdeck/loops.md`, implementing the next step, and updating the memory files when done. No cloud, no database, no lock-in.

## Why?

Developers working with AI tools lose project context constantly — purpose, decisions, constraints, current goals — all scattered across chat histories and notes. When you switch projects, you re-explain everything from scratch.

LoopDeck solves this by storing project memory **inside the repository** using a simple, file-based convention:

```
your-repo/
├── .loopdeck/
│   ├── project.yaml        # name, description, status, timestamps
│   ├── decisions.md        # architectural decision log
│   ├── loops.md            # current loop + next steps checklist
│   ├── current-loop.md     # short summary of the in-flight loop
│   └── sessions/
│       ├── active.jsonl    # live agent transcript
│       └── archive-*.jsonl # rotated past conversations
├── .claude/                # Claude Code — skills, hooks, settings
│   ├── skills/
│   └── settings.local.json
└── .agents/
    └── skills/             # Codex project skills
```

Any tool — AI agent, CLI, editor — can read and write these files. LoopDeck provides the desktop UI and the agent runtime.

## What It Does

**Project discovery & memory**
- 🔍 **Scans** a directory recursively for repositories (`.git`, `Cargo.toml`, `package.json`, `go.mod`, and more)
- 📦 **Imports** selected repos; bootstraps `.loopdeck/project.yaml` (description parsed from `README.md`), seeds skills for Claude and Codex, and configures Claude hooks
- 🗂️ **Maintains** a local registry at `~/.config/loopdeck/config.yaml`
- ✏️ **Edits** descriptions inline, or regenerates from README
- 📊 **Shows** git freshness (last commit, uncommitted counts) and relative timestamps
- 🔗 **Opens** projects in Finder or Terminal directly from the UI

**Agent loop**
- 🤖 **Runs the `claude` CLI** as a managed subprocess per project, streaming tokens to the UI as they arrive
- ▶️ **"Start loop"** builds a prompt from the first unchecked step in `loops.md` and kicks off a fresh conversation; **free-form chat** continues the existing one
- 🧠 **Resumes context** — after a restart, the agent re-spawns with `--resume <session_id>` so the model's context is restored
- 💬 **Surfaces `AskUserQuestion` cards and manual tool-approval prompts** inline in the chat, with "Always allow" rules persisted to `.claude/settings.local.json`
- ⏹️ **Graceful Stop** interrupts the in-flight turn while keeping the live process (and its context) alive
- 📜 **Conversation history** — transcripts are archived per conversation and can be reopened, promoted back to active, or reset

**Composer**
- `@` **-mention autocomplete** for files and folders, with ranked fuzzy search across the whole tree
- `/` **-skill discovery** lists the active harness's installed skills (`.claude/skills` for Claude, `.agents/skills` for Codex)

## Quick Start

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (stable)
- The `claude` CLI on your `PATH` (for the agent loop)

### Install & Run

```bash
# Install frontend dependencies
npm install

# Launch in development mode (hot reload)
npm run tauri dev

# Build for production
npm run tauri build
```

Before starting a loop, open **Settings** and configure the agent (auth token, model, etc.). The config is stored at `~/.config/loopdeck/config.yaml`.

### Verify

```bash
# Run Rust tests (194 tests across 12 modules)
cd src-tauri && cargo test

# Rust lint
cargo clippy

# Frontend type-check + build
npx tsc --noEmit && npm run build
```

## Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri v2 |
| Frontend | Vite + React 19 + TypeScript |
| Routing | TanStack Router |
| State | Zustand |
| UI primitives | shadcn/ui (Radix) + Tailwind CSS v4 |
| Markdown | react-markdown + remark-gfm + rehype-highlight |
| Toasts | Sonner |
| Backend | Rust (tokio for async process I/O) |
| Agent runtime | `claude` CLI subprocess over NDJSON stdin/stdout |
| Storage | YAML + JSONL files on disk — no database |

### Architecture

```
┌──────────────────────────────────────────────────────┐
│  React 19 + Vite + TanStack Router + Tailwind v4     │
│  ┌──────────┐  ┌─────────────┐  ┌────────────────┐   │
│  │ Zustand  │  │ Typed IPC   │  │ Tauri Channel  │   │
│  │ stores   │  │ wrappers    │  │ (live streams) │   │
│  └──────────┘  └──────┬──────┘  └────────┬───────┘   │
├───────────────────────┼───────────────────┼──────────┤
│  Tauri v2 IPC Bridge  │                   │          │
├───────────────────────┼───────────────────┼──────────┤
│  Rust Backend         ▼                   ▼          │
│  ┌─────────────────────────────────────────────────┐ │
│  │ AppState                                        │ │
│  │  • Mutex<GlobalConfig>          (registry)      │ │
│  │  • per-project ClaudeSession    (live process)  │ │
│  │  • pending question/approval/interrupt slots    │ │
│  └─────────────────────────────────────────────────┘ │
│  scanner project memory git │ agents conversation    │
│  config permission  skills  │ claude_session logging │
├──────────────────────────────────────────────────────┤
│  File System                                         │
│  .loopdeck/ + .claude/ + ~/.config/loopdeck/         │
└──────────────────────────────────────────────────────┘
```

Per-project agent sessions use a two-layer lock: an outer `std::sync::Mutex` guards the session map for microseconds, while an inner `tokio::sync::Mutex` per project serializes turns (one stdin, one process). Different projects take different inner locks, so they run in true parallel.

## Project Structure

```
loopdeck/
├── src/                       # React frontend (TypeScript)
│   ├── components/
│   │   ├── dashboard/         # project cards, empty state
│   │   ├── detail/            # ProjectDetail + tab panels
│   │   │   ├── AgentPanel.tsx     # streaming agent surface
│   │   │   ├── Chat.tsx           # message list + composer
│   │   │   ├── FileMentionMenu.tsx   # @-mention autocomplete
│   │   │   ├── SkillMenu.tsx         # /-skill discovery
│   │   │   ├── TaskPanel.tsx         # live task events
│   │   │   └── DecisionsPanel/LoopsPanel/EditDescription
│   │   ├── agent/ activity/ decisions/ loops/
│   │   ├── import/ settings/ layout/
│   │   ├── shared/            # Markdown, ConfirmDialog, StatusBadge
│   │   └── ui/                # shadcn/ui primitives
│   ├── router.tsx             # routes: / /activity /agent /decisions
│   │                          #   /loops /settings /import /project/$p
│   ├── store/                 # appStore, streamingState, pendingInteractions
│   ├── hooks/                 # useProjects (async IPC)
│   ├── lib/                   # typed Tauri wrappers, theme, time, utils
│   └── types/                 # TS interfaces mirroring Rust structs
├── src-tauri/                 # Rust backend
│   └── src/
│       ├── commands.rs        # 33 Tauri IPC handlers + AppState
│       ├── config.rs          # GlobalConfig, XDG paths, AgentConfig
│       ├── scanner.rs         # repo discovery (walkdir + marker files)
│       ├── project.rs         # .loopdeck/ bootstrap, README parsing
│       ├── memory.rs          # decisions.md / loops.md parsers
│       ├── git.rs             # git freshness detection
│       ├── agents.rs          # claude NDJSON protocol, events, responses
│       ├── claude_session.rs  # subprocess lifecycle, streaming read loop
│       ├── conversation.rs    # transcript load/archive/resume, history
│       ├── permission.rs      # tool-approval policy + allow rules
│       ├── skills.rs          # install skills for both harnesses + Claude hooks
│       ├── logging.rs         # tracing + file appender
│       └── error.rs           # AppError (thiserror + serde)
├── templates/                 # bundled skills + hooks, seeded on import
│   ├── skills/                # orchestrator + rust/tauri/go/ios experts
│   └── hooks/                 # memory-write, stop, orchestrator-start
├── docs/                      # PRDs, research notes, postmortems
└── .loopdeck/                 # LoopDeck's own project memory (dogfooding)
```

## IPC Commands

All 33 commands are typed on the frontend — raw `invoke()` is never called from components.

**Discovery & files**
| Command | What it does |
|---------|-------------|
| `scan_directory` | Recursively discover repos by marker files |
| `list_dir_entries` | List a project subdir for `@`-mention autocomplete |
| `search_project_files` | Ranked fuzzy search across the project tree |
| `list_skills` | List installed skills for `/`-discovery |

**Project registry**
| Command | What it does |
|---------|-------------|
| `import_project` | Bootstrap `.loopdeck/` + `.claude/`, register project |
| `list_projects` / `get_project` | List / fetch registered projects |
| `update_description` / `regenerate_description` | Edit / re-parse description |
| `remove_project` / `rescan_project` | Drop from registry / refresh git info |
| `open_in_finder` / `open_in_terminal` | Reveal in file manager / terminal |

**Memory**
| Command | What it does |
|---------|-------------|
| `get_decisions` | Parse `.loopdeck/decisions.md` |
| `get_loops` | Parse `.loopdeck/loops.md` status |

**Agent config & execution**
| Command | What it does |
|---------|-------------|
| `get_agent_config` / `set_agent_config` | Read / persist agent settings |
| `agent_start_loop[_streaming]` | Fresh conversation from next `loops.md` step |
| `agent_send_message[_streaming]` | Continue the existing conversation |
| `agent_interrupt` | Graceful Stop (keeps the live process) |
| `agent_reset_session` | Drop process + archive transcript |
| `agent_is_busy` | Report in-flight turn state |

**Conversations**
| Command | What it does |
|---------|-------------|
| `agent_get_conversation` | Load active transcript |
| `agent_list_conversations` | List active + archived summaries |
| `agent_get_conversation_by_id` | Open a past conversation read-only |
| `agent_promote_to_active` | Resume an archived conversation |

**Approvals & questions**
| Command | What it does |
|---------|-------------|
| `agent_answer_question` | Resolve a pending `AskUserQuestion` card |
| `agent_answer_permission` | Allow/Deny a pending tool-approval card |
| `agent_add_allow_rule` | Persist "Always allow" rule to `.claude/` |
| `agent_pending_question` / `agent_pending_permission` | Poll for pending cards |

## Scope

✅ Discover local repositories
✅ Create `.loopdeck/` memory structure (`project.yaml`, `decisions.md`, `loops.md`, `current-loop.md`)
✅ Generate project descriptions from README.md
✅ Maintain local registry at `~/.config/loopdeck/config.yaml`
✅ Run an on-device agent loop against project memory, with streaming chat
✅ `AskUserQuestion` + manual tool approvals, "Always allow" rules
✅ Conversation history with archive, resume, and promote
✅ Composer `@`-mention file autocomplete and `/`-skill discovery
✅ Bundled skill + hook templates seeded into `.claude/` on import
✅ 194 Rust tests across 12 modules; full frontend type-check

Not in scope today: cloud sync, team collaboration, multi-agent orchestration.

## What's Next

- **Multi-project dashboarding** — cross-project loop status, activity feed
- **Richer task panel** — surface `TodoWrite`-style task events live during a turn
- **Loop automation** — scheduled/triggered loops, PR handoff
- **Additional agent backends** — Codex / local models alongside `claude`

## License

MIT
