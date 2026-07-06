# Decisions

## 2026-06-22 — Use Tauri v2 for desktop shell
- **Status**: accepted
- **Context**: We needed a cross-platform desktop shell for LoopDeck. Electron was the default, but bundle size and the need for a performant filesystem scanner pushed us to evaluate alternatives.
- **Consequences**: Must maintain Rust + TypeScript across the IPC boundary. No Node.js main process — all system operations go through Tauri commands. Smaller bundle, better FS performance.

## 2026-06-22 — Use Zustand for frontend state management
- **Status**: accepted
- **Context**: React Context + useReducer was verbose for cross-component state. Redux added too much boilerplate. Zustand offered minimal API with selector-based subscriptions and no provider wrapping.
- **Consequences**: All state lives in `src/store/appStore.ts`. Components subscribe via selectors. No Redux DevTools dependency (though Zustand supports it optionally).

## 2026-06-22 — Store project memory inside repos, not a database
- **Status**: accepted
- **Context**: Other AI project managers (Codex, Cursor rules) store context in proprietary formats or cloud databases. LoopDeck needed to be local-first and repo-portable so context travels with the code.
- **Consequences**: `.loopdeck/` directory lives in each repo. Global registry at `~/.config/loopdeck/config.yaml` is an index, not source of truth. Files are Markdown + YAML for human + AI readability.

## 2026-06-22 — Markdown for agent memory files (decisions.md, loops.md)
- **Status**: accepted
- **Context**: For V2 agent memory, we considered YAML, JSON, and SQLite. Markdown was chosen because it's human-readable, AI-friendly (all LLMs understand Markdown), and doesn't require a parser library on the agent side.
- **Consequences**: The Rust backend uses regex-free line-by-line Markdown parsing. Loosely structured — the conventions (## headings, **Key**: Value bullets) are enforced by convention, not schema validation.

## 2026-06-22 — Sidebar tab navigation in ProjectDetail instead of single scroll view
- **Status**: accepted
- **Context**: V2 adds Decisions and Loops tabs alongside the existing Overview. A single scroll view would be too long. Tabs allow clear separation of concerns.
- **Consequences**: ProjectDetail now has a left sidebar (180px) with tab buttons and a right content panel. Overview becomes one of three tabs.

## 2026-06-22 — Dual Stop hooks for agent memory auto-write
- **Status**: superseded
- **Context**: We need AI agents to update .loopdeck/ files automatically at session end. Two approaches: prompt-based (AI writes rich content) and shell script (mechanical fallback). Both are complementary.
- **Consequences**: `.claude/settings.local.json` has a Stop hook with both a prompt and a shell command. The prompt gives richer content; the shell script ensures the files always exist.

## 2026-06-22 — Stop hook dirty-flag gating (temporary workaround)
- **Status**: accepted
- **Context**: The Stop hook was firing on every session end — even pure Q&A with no code changes — causing repetitive "LOOPDECK MEMORY UPDATE REQUIRED" nags. Added a PreToolUse hook that creates `.claude/.session-dirty` on Edit/Write, and the Stop hook only emits the reminder when that flag exists.
- **Consequences**: Cleaner sessions — no nag on Q&A. But this is a **temporary workaround**. The real fix is to move all agent control (CLAUDE.md, skills, hooks, memory conventions) into the LoopDeck app itself, where it can intelligently decide when to prompt. See parking lot in loops.md.

## 2026-06-23 — Moved LoopDeck memory hooks to global config with .loopdeck/ gating
- **Status**: accepted
- **Context**: The memory-update hooks (PreToolUse dirty flag + Stop reminder/heartbeat) were project-local to the loopdeck repo. Any LoopDeck-tracked project with a `.loopdeck/` directory should get the same memory-update automation.
- **Consequences**: Scripts live at `~/.claude/hooks/loopdeck-stop-hook.py` and `loopdeck-memory-write.sh`. Both gate on `.loopdeck/` directory existence — in non-LoopDeck projects they exit silently. Hooks configured in global `~/.claude/settings.json`. Project-local `settings.local.json` cleaned up (hooks removed, permissions kept for dev use).

## 2026-06-23 — Fixed Stop hook matcher and dirty-flag race condition
- **Status**: accepted
- **Context**: The Stop hooks were configured with `"matcher": ""` which only matches an empty string — it never matches real stop reasons like `"finished"` or `"interrupted"`. Additionally, the Python reminder hook consumed `.session-dirty` (deleted it), starving the shell heartbeat fallback. Both issues meant the memory-update reminder never fired.
- **Consequences**: Changed matcher to `".*"` (match any stop reason). Python script now reads `.session-dirty` without deleting it; only the shell fallback consumes the flag. Both Stop hooks can now see the dirty flag and both can fire.

## 2026-06-23 — Git-age-based project status auto-classification
- **Status**: accepted
- **Context**: We needed project status (Active/Warning/NonActive) to reflect reality, not manual user input. The `rescan_project` command already refreshes git info — it's the natural place to derive status.
- **Consequences**: Status is derived from last commit date during rescan: 0–6 days → Active, 7–30 days → Warning, 30+ days → NonActive. Fallback to last_modified if no commits. Archived remains a manual status. Frontend color-codes status badges (green/yellow/red/gray). Imported projects default to Active until first rescan.

## 2026-06-22 — Stop hook must use command type, not prompt type
- **Status**: accepted
- **Context**: The initial Stop hook used `type: "prompt"` to remind the AI to update .loopdeck/ files. This silently failed because Claude Code's hook system only allows `type: "prompt"` on PreToolUse, PostToolUse, and PermissionRequest events — NOT on Stop. Stop only supports `type: "command"`.
- **Consequences**: Replaced the prompt hook with a `type: "command"` hook that runs python3 to output JSON with `hookSpecificOutput.additionalContext`. This injects the memory update reminder into the model's context on next wake-up. The shell script fallback (Approach B) was unchanged — it was already a valid command hook. Both hooks now fire correctly on Stop.

## 2026-06-24 — Adopt Tailwind CSS v4 + OKLCH dark palette from AI Project Command
- **Status**: accepted
- **Context**: LoopDeck's UI used plain CSS with BEM naming and a GitHub-dark hex palette. The AI Project Command app (reference UI) uses a more polished design: Tailwind CSS v4, OKLCH color space, layered surface backgrounds, gradient accent lines, and Inter+JetBrains Mono fonts. Adopting this design system makes LoopDeck look professional and consistent.
- **Consequences**: All component CSS files deleted — styles now live in Tailwind utility classes. Single `src/styles.css` with OKLCH tokens and Tailwind config. AppShell changed from top header bar to sidebar navigation layout. Added PageHeader component for sticky view headers. Design tokens (colors, radii, fonts, shadows) match AI Project Command exactly.

## 2026-06-24 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on LoopDeck development.


## 2026-06-27 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on LoopDeck development.


## 2026-06-27 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on LoopDeck development.

