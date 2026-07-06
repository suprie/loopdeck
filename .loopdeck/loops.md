# Loops

## Current
- **Started**: 2026-06-24
- **Goal**: UI restyling — match AI Project Command aesthetic (Tailwind CSS v4, OKLCH dark palette, sidebar layout, gradient accents, Inter+JetBrains Mono fonts)
- **Status**: in_progress

## Next Steps
- [ ] Add Agent Runner view (`/agent`) — terminal-based AI agent runner UI
- [ ] Add Activity Feed view (`/activity`) — chronological event feed
- [ ] Add standalone Decisions page (currently only accessible via project detail)
- [ ] Add standalone Next Loop page (currently only accessible via project detail)
- [ ] Add TanStack Router for proper client-side routing (replace Zustand view switching)
- [ ] Add shadcn/ui component library for polished UI primitives
- [ ] Add keyboard shortcut (⌘K) command palette
- [ ] Add unit tests for status derivation logic (0 days, 6 days, 7 days, 30 days, 31+ days, no git)
- [ ] Run `npm run tauri dev` to verify UI renders correctly with color-coded status badges
- [ ] Consider: should status also update on `list_projects` (not just rescan)?
- [ ] Cross-platform testing

## Parking Lot
- [ ] **Move agent control into LoopDeck app** — When LoopDeck can spawn/manage AI agents from within the app (not just the terminal), it should own all agent configuration: CLAUDE.md, skills, hooks, and memory conventions. The current `.claude/settings.local.json` hooks (PreToolUse dirty flag, Stop hook reminder) are temporary workarounds that only work in the Claude Code terminal context. Once LoopDeck controls the agent runtime, it can intelligently decide when to prompt for memory updates, apply project-specific instructions, and manage skills — without relying on external hook files.

## History

### 2026-06-22 — V2 Agent Memory Layer
- **Status**: completed
- **Completed**: 2026-06-22

Full V2 agent memory layer implemented across 4 phases:

**Phase 1 — Backend (rust-expert):** memory.rs with lenient Markdown parser for decisions.md
(architectural decision records) and loops.md (current loop, next steps, history). Two new IPC
commands: get_decisions, get_loops. 22 unit tests covering edge cases (em dash, hyphen,
empty files, missing headings, partial file creation).

**Phase 2 — Frontend (vite-senior-engineer):** DecisionsPanel and LoopsPanel components
with loading/empty/error states. Sidebar tab navigation in ProjectDetail (Overview |
Decisions | Loops). All matches existing Zustand + typed IPC conventions.

**Phase 3 — Agent Convention:** Project-local .claude/skills/orchestrator SKILL.md extending
the global orchestrator with .loopdeck/ write conventions. CLAUDE.md updated with memory
convention. settings.local.json Stop hook with dual approach: command hook with
hookSpecificOutput.additionalContext (injects memory reminder into model context) and shell
script (mechanical heartbeat fallback). Initial implementation used `type: "prompt"` which
silently failed — fixed by switching to `type: "command"` with JSON output. Hook verified
working via pipe-test and jq validation.

**Phase 4 — Review:** rust-code-reviewer and vite-senior-engineer review completed. One
medium finding (leading-newline split pattern) fixed. 5 additional edge case tests added.
Final: 52 Rust tests passing, TypeScript clean, 12 IPC commands registered.

Files created: memory.rs (610 lines), DecisionsPanel.tsx, LoopsPanel.tsx (both ~120 lines),
DecisionsPanel.css, LoopsPanel.css, orchestrator SKILL.md, loopdeck-memory-write.sh.
Updated: ProjectDetail.tsx/CSS (sidebar nav), types/index.ts, tauri.ts, lib.rs, commands.rs,
CLAUDE.md, settings.local.json, .loopdeck/decisions.md (6 decisions), .loopdeck/loops.md.

### 2026-06-22 — V2 Agent Memory Backend
- **Status**: completed
- **Completed**: 2026-06-22

Created memory.rs with Markdown parser for decisions.md and loops.md. Two new IPC commands:
get_decisions and get_loops. 18 new tests. All 47 tests passing.

### 2026-06-22 — V1 Gaps
- **Status**: completed
- **Completed**: 2026-06-22

Fixed 4 V1 gaps: scan_depth enforcement, last_opened on dashboard, detected_stack +
description_preview on import, rescan_project command. 30→30 tests (added max_depth test).

### 2026-06-22 — V1 Core
- **Status**: completed
- **Completed**: 2026-06-22

Built the scanner, .loopdeck/ bootstrap, project config registry, and full React UI.
10 IPC commands: scan, import, list, get, update_description, remove, open_in_finder,
open_in_terminal, regenerate_description, rescan_project. 30 Rust tests passing.
