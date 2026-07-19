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


## 2026-07-03 — Per-content-block streaming granularity for `ClaudeEvent`

- **Status**: accepted
- **Context**: The `--output-format stream-json` stream produces one NDJSON line per `assistant` message, and each message can contain multiple `content` blocks (text, thinking, tool_use). We had a choice: emit one `ClaudeEvent` per NDJSON line (the entire message), or break messages apart and emit one event per content block.
- **Consequences**: Chose per-content-block emission (`TextDelta`, `ThinkingDelta` per block). This gives the frontend the most natural granularity — each delta is a complete text fragment ready to render. The accumulator (`ResponseAccumulator`) still processes the full message in one `ingest_event` call, keeping aggregation consistent with the batch path.

## 2026-07-03 — Separate batch and streaming `send_message` paths (not refactored into one)

- **Status**: accepted
- **Context**: After adding `send_message_streaming`, we considered refactoring `send_message` to call `send_message_streaming` with a no-op or dropped channel, so the read loop lives in one place. The two paths share `ResponseAccumulator` but differ meaningfully: the batch path is simpler (just accumulate), while the streaming path adds per-block event emission and a terminal `Result` event.
- **Consequences**: Kept them separate (~15 lines of stdin-write duplication). The batch path avoids allocating channel overhead and remains the simpler, easier-to-reason-about code path. The streaming path adds exactly the delta-emission logic and nothing else. If a third variant is ever needed, extraction becomes worthwhile — for two, the duplication is cheaper than the abstraction.

## 2026-07-03 — Best-effort channel sends in streaming (closed channel is not an error)

- **Status**: accepted
- **Context**: The frontend may close the Tauri `Channel<ClaudeEvent>` mid-turn (e.g., navigating away from the Agent tab). The Rust side could either (a) treat a closed channel as fatal and abort the turn, or (b) silently drop the send and let the turn complete.
- **Consequences**: Chose (b) — best-effort sends with `let _ = channel.send(…)`. Rationale: the transcript is always recorded regardless (it's written after the turn completes in `send_and_record_streaming`), and aborting mid-turn would orphan a running claude process with an incomplete stdin stream. The user can always see the result in the transcript when they return.

## 2026-07-03 — Streaming command returns `()` not `AgentResponse`

- **Status**: accepted
- **Context**: `agent_send_message` returns `AgentResponse` as the Tauri command return value, which the frontend awaits as a Promise. For streaming, the frontend gets data through the Channel, not the return value. We could return `AgentResponse` as well (giving two sources of truth), or return `()`.
- **Consequences**: Chose `()` — the terminal `ClaudeEvent::Result` carries the full aggregated response inline as the last event on the channel. This is a single source of truth: the frontend listens to the channel and uses the `Result` event to finalize its UI state. Returning `AgentResponse` as well would create ambiguity about which payload to trust.

## 2026-07-03 — Frontend streaming chat UI uses Channel events as single source of truth

- **Status**: accepted
- **Context**: After adding `agent_send_message_streaming` (Rust) and `ClaudeEvent` types (TS), the `AgentPanel` still used batch APIs exclusively — users saw a spinner for the full turn duration. Building a streaming UI required deciding how the frontend detects turn completion, errors, and final state.
- **Consequences**: The Tauri `Channel<ClaudeEvent>` is the single source of truth for turn state. The `invoke` Promise is only used for infra-level error catching (timeout, no config, spawn failure). Model-level errors (`is_error: true`) are surfaced from the `ClaudeEvent::Result` event, not from Promise rejection. This is consistent with the existing decision that streaming commands return `()` not `AgentResponse`. The `StreamingBubble` component accumulates `TextDelta`/`ThinkingDelta` events in real time and transitions to a "complete" state when the terminal `Result` event arrives. A `mountedRef` guard prevents post-unmount state updates; a `resultHandled` flag prevents double-reload if the invoke Promise resolves before the last channel event.

## 2026-07-03 — Streaming "Start next loop" variant added for UI consistency

- **Status**: accepted
- **Context**: The existing `agent_start_loop` was batch-only. Adding streaming to the free-form send but not to Start-next-loop would create an inconsistent UX — the first turn of every session would show a spinner while follow-ups streamed. We could either (a) have the frontend build the prompt and call `agent_send_message_streaming`, or (b) add `agent_start_loop_streaming` in Rust.
- **Consequences**: Chose (b) — `agent_start_loop_streaming` mirrors the batch `agent_start_loop` exactly (same prompt-building logic via `build_next_loop_prompt`, same transcript recording via `send_and_record_streaming`) and differs only in how the response reaches the UI. This keeps prompt-building logic in one place (Rust) and avoids duplicating `build_next_loop_prompt` / `next_unchecked_loop_step` on the frontend. The two streaming commands (`agent_start_loop_streaming` and `agent_send_message_streaming`) share the same `send_and_record_streaming` pipeline.

## 2026-07-03 — Extracted presentational `Chat` component from `AgentPanel`

- **Status**: accepted
- **Context**: After the streaming AgentPanel was built, all rendering logic (`TurnBubble`, `StreamingBubble`, `ThinkingBlock`, composer, error banner, empty state) was inline in AgentPanel alongside Channel lifecycle management — the file mixed orchestration and presentation concerns. For reusability (future `/agent` standalone view, potential mobile port) and testability, rendering needed its own module.
- **Consequences**: `Chat.tsx` is a pure presentational component with zero Tauri or Channel imports. All streaming state (`streamingText`, `streamingThinking`, `streamingResult`, `busy`, `error`) flows in via props; user actions flow out via callbacks (`onSend`, `onClearError`). `AgentPanel.tsx` retains sole ownership of the `Channel<ClaudeEvent>` lifecycle, delta accumulation, transcript persistence (`reload()`), and toolbar buttons. This one-way data flow makes `Chat` independently reusable and testable, while `AgentPanel` remains the single source of truth for streaming orchestration. The `streamingResult` prop gives `Chat` enough context to show usage/duration meta in the transient window before the transcript reload replaces the streaming bubble with the persisted turn.

## 2026-07-03 — Agent Runner uses terminal theme, not Chat bubbles

- **Status**: accepted
- **Context**: The Agent Runner view needed a standalone agent interface with a project selector. We considered reusing the `Chat` component (which already handles streaming rendering) but `Chat`'s bubble-based, avatar-heavy design doesn't fit a "terminal" aesthetic. The Agent Runner is meant to feel like a developer tool — a tmux-for-AI-agents — not a chat app.
- **Consequences**: `AgentRunner.tsx` renders its own terminal-themed output (monospace font, dark background `oklch(0.13 0.01 270)`, prompt indicators `❯`, flat line-by-line output, tool-call diamonds `◈` in warning color). It does NOT import `Chat`. However, it does reuse the same streaming orchestration pattern as `AgentPanel` — Channel → `onmessage` → delta accumulation → Result → reload. The two components are rendering-siblings but orchestration-cousins: same approach to Channel lifecycle, different visual output. If a third agent view is ever needed, the shared streaming logic should be extracted into a `useStreamingTurn` hook.

## 2026-07-03 — Activity Feed merges all sources into single chronological timeline

- **Status**: accepted
- **Context**: The Activity Feed needs to show agent turns, decisions, and loop completions across all projects. We considered rendering separate sections per data source (Conversations, Decisions, Loops) within the feed, vs merging everything into one sorted timeline.
- **Consequences**: Chose a single merged `ActivityEvent[]` discriminated by `kind` (`turn_user` | `turn_assistant` | `turn_error` | `decision` | `loop_completed`). One sort pass, one date-grouping pass, one render loop. The alternative (separate sections) would fragment chronology and require users to mentally merge timelines. Date-only sources (decisions, loops) get synthesised midnight UTC timestamps so they sort within the correct date bucket. Per-source fetching is best-effort — a missing transcript doesn't prevent decisions/loops from appearing.

## 2026-07-03 — Standalone Decisions + Loops use expand-in-place cards, not detail pages

- **Status**: accepted
- **Context**: Both the standalone Decisions and Loops views aggregate data across all projects. We could either (a) use click-to-expand cards showing detail inline, or (b) navigate to a per-item detail page (requiring selected-decision-ID / selected-loop-path in Zustand state). With TanStack Router not yet adopted, option (b) would add routing-like state management prematurely.
- **Consequences**: Chose expand-in-place cards for both views. Clicking a decision card expands it inline to show full context + consequences. Clicking a loop card expands it to show next-steps checklist + history. The ChevronDown icon rotates 180° to indicate state. This keeps the list visible for context while showing detail, and defers the routing complexity until TanStack Router is adopted.

## 2026-07-03 — TanStack Router: memory history, Zustand for data only

- **Status**: accepted
- **Context**: The app used a Zustand `currentView` string + conditional render (`{currentView === "dashboard" && <Dashboard />}`) for view switching. This worked but had no URL-based routing, no type-safe params, and no nested layout support. We needed proper client-side routing before adding more views.
- **Consequences**: Adopted `@tanstack/react-router` v1 with `createMemoryHistory` (no browser URL bar in Tauri). Routes: `/`, `/activity`, `/agent`, `/decisions`, `/loops`, `/settings`, `/import`, `/project/$projectPath`. The root route is a layout component (`AppShellLayout`) with sidebar + `<Outlet />`. Zustand retains data state (`projects`, `selectedProject`, `detailTab`, `pendingAgentStart`) but no longer owns the current view. Project filesystem paths are URI-encoded in the route param (`encodeURIComponent`/`decodeURIComponent`). Navigation uses `<Link>` (sidebar) and `useNavigate()` (programmatic). The persisted Zustand slice was reduced from `{currentView, selectedProject, detailTab}` to `{selectedProject, detailTab}` — the router's memory history handles the current location.

## 2026-07-02 — Session heartbeat
## 2026-07-06 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on LoopDeck development.

## 2026-07-08 — Spec layer in `docs/epics/`, runtime layer in `.loopdeck/`

- **Status**: accepted
- **Context**: LoopDeck executed work as a flat list of loops in `.loopdeck/loops.md`, mixing plan + execution in one file. Planning past ~10 items broke down — no grouping, no ownership boundary, no reviewable spec. The original proposal put epics in `.loopdeck/epics.md`, but that would conflate the plan (intention, authored deliberately) with runtime state (what the app writes during execution) and treat the plan as mutable app state that drifts.
- **Consequences**: Epics and PRDs live under `docs/epics/<slug>/` (one directory per epic, co-located PRDs), committed to git and reviewable in PRs. `.loopdeck/` stays the runtime layer — current loop, history, decisions. The bridge is a single promote-to-loop action that writes a PRD checklist item into `loops.md ## Current` carrying `**Epic**`/`**PRD**` back-references. The agent stays unaware of the hierarchy (no change to `build_next_loop_prompt`); the app owns the spec layer, the human owns the commit. Two views of the same work (PRD checklist vs. History) will drift — 0.2.0 makes that drift visible rather than auto-syncing. Full spec: `docs/epics/support-project-management/`.

## 2026-07-08 — YAML frontmatter for spec files, bullets for runtime files

- **Status**: accepted
- **Context**: The first epic format draft used `**Milestone**: 0.2.0` bullets in the body, mirroring `decisions.md`/`loops.md`. But epics need to be *indexed* — grouped by milestone, filtered by status — which is structurally the same job SKILL.md solves with YAML frontmatter (a small structured header for discovery, prose body for content). Bullets conflate the index layer with the content layer and force the parser to line-scan for fields that should be queryable.
- **Consequences**: Spec-layer files (`docs/epics/**/*.md`) carry YAML frontmatter with index fields (`title`, `slug`, `milestone`, `status`, `started`, `completed`, `owner`, `description`); the body is pure prose. Runtime-layer files (`.loopdeck/*.md`) keep the `**Field**: value` bullet convention — they're agent-written and lenient. The `/epics` view groups by `milestone` via a frontmatter query, no body parsing. The format difference reinforces the layer separation: structured where humans index, lenient where agents write. The back-reference the app writes into `loops.md` on promote stays as bullets, because it's writing into a runtime file. `epic.rs` uses `serde_yaml` (already in the dep tree) for frontmatter and line-scans only the `### Phase` sections. `milestone` is quoted (`"0.2.0"`) because YAML parses unquoted `0.2.0` as the float `0.2`.

## 2026-07-08 — Split `loopdeck-orchestrator` into runner + author + memory

- **Status**: accepted
- **Context**: The single `loopdeck-orchestrator` skill conflated runtime mechanics (how to execute a loop) with strategy (how to plan a project). Once the spec layer lives in `docs/epics/`, the strategy content collides with the app-owned plan — the agent would keep re-decomposing plans the human already authored. The skill needed stripping to mechanics.
- **Consequences**: Three focused skills replace one: `loopdeck-loop-runner` (mechanics + a new read-context rule that follows a loop's epic/prd back-reference as context, not mandate), `loopdeck-epic-author` (human-invoked drafting aid that elaborates a coarse goal via clarifying questions into reviewable drafts — never commits or promotes), `loopdeck-memory` (decisions.md + loops.md write conventions). Authoring intelligence moves to app-invoked dialogues in 0.3.0, not standing skills. Skill descriptions must be phrased on-demand ("when the user wants to draft") to avoid recreating autonomous-planning drift.

## 2026-07-08 — Managed-skills refresh via version manifest

- **Status**: accepted
- **Context**: `copy_skills` skips any skill whose `SKILL.md` already exists, to preserve user customizations. This made the orchestrator split unreachable on existing projects — they'd keep the fat self-directing version forever. Real fix is runtime skill injection (Parking Lot: "Move agent control into LoopDeck app"), but that shouldn't gate 0.2.0.
- **Consequences**: Stopgap: a `.claude/skills/.loopdeck-manifest.json` records installed skills + app version. The `loopdeck-` prefix is the ownership boundary — app-managed skills are overwritten when app version advances; user-owned skills (no prefix) are never touched. A one-time migration removes the legacy `loopdeck-orchestrator` directory, logged. Users customize by copying to a non-prefixed name. The convention carries forward to the runtime-injection model when it lands.

## 2026-07-10 — Auth token stored in OS keychain, not config.yaml

- **Status**: accepted
- **Context**: The agent auth token (an API key for the model provider) was stored in plaintext at `~/.config/loopdeck/config.yaml`. The threat model isn't a remote attacker — it's local exposure: the file gets swept into backups, cloud-sync folders, accidental commits, or read by another local process on a shared machine. The OS credential store (macOS Keychain / Windows Credential Manager / Linux Secret Service) provides OS-managed encryption and per-item access control for free, and is the standard place desktop apps keep secrets. `chmod 600` on the file was the interim floor already noted in the audit, but it doesn't help against backups/sync and only slows a same-user process.
- **Consequences**: The token now lives in the keychain via the `keyring` crate (`secrets.rs`); it is never written to `config.yaml` — only `base_url` / `model` / `effort` are. `get_agent_config` never returns the plaintext token to the renderer; it returns a `has_auth_token` presence flag so the Settings UI can show a masked "token stored" affordance. The token is resolved from the keychain at spawn time into a local `AgentConfig` (via `resolve_agent_config`) and set as the child's `ANTHROPIC_AUTH_TOKEN` env var — it is not held on the long-lived `Mutex<GlobalConfig>`. A one-time startup migration (`migrate_auth_token_to_keychain`) moves any existing plaintext token to the keychain and scrubs the file. `config.yaml` is still tightened to `0600` on every save as defense-in-depth, and if the keychain is unavailable (e.g. headless Linux with no D-Bus) the token stays in the `0600` file as the interim floor rather than being dropped. A new `clear_auth_token` command + Settings button lets the user revoke a stored token. The `AgentConfig.auth_token` field is retained on the wire type (the frontend sends a new token through it on save) but is always `None` on read.

## 2026-07-10 — Offload blocking I/O in Tauri commands to `spawn_blocking`

- **Status**: accepted
- **Context**: Four `async` Tauri commands — `scan_directory`, `import_project`, `list_projects`, `rescan_project` — performed blocking I/O directly on the tokio worker thread: recursive `walkdir` tree walks, per-repo `git` subprocess spawns (`git log` / `status` / `diff`), and file reads. A `scan_directory` against a large home directory, or a `list_projects` over many registered repos, can block for seconds. Tokio's multi-threaded runtime runs each `async fn` on one worker until it yields; a blocking call parked on that worker stalls every other IPC command sharing it (the whole UI goes unresponsive for the duration). The runtime's dedicated blocking pool exists precisely for this.
- **Consequences**: The heavy I/O in each command now runs inside `tokio::task::spawn_blocking`, which moves it onto the blocking thread pool and frees the worker to service other commands while it runs. Crucially, the `Mutex<GlobalConfig>` lock is no longer held *across* that I/O — each command snapshots what it needs under a brief lock, does the blocking work lock-free on the pool, then re-locks briefly to apply + `save()`. `list_projects` keys its refresh results by path so the apply pass stays aligned even if the registry changed between snapshot and apply (a project added/removed by a concurrent command simply doesn't match and is left untouched). The closures return their `AppError`s directly (the type is `Send`), so failure paths are unchanged; a join failure (task panic/cancellation) maps to a new `AppError::BlockingTask` variant rather than leaking a raw `tokio::task::JoinError`. Command signatures and return types are unchanged, so the frontend IPC wrappers need no edits. `config.save()` (a brief atomic file write) is intentionally kept on the worker under the lock — only the multi-second walkdir/git work was the problem. The same blocking-on-worker anti-pattern remains in `claude_session.rs` `Drop` and is tracked as its own next step.

## 2026-07-10 — Reap claude child off the tokio worker in `Drop` via `spawn_blocking`

- **Status**: accepted
- **Context**: `ClaudeSession::drop` reaped the child with a synchronous `poll_reap` helper that loops `try_wait()` paced by `thread::sleep` — up to 7s total (5s graceful EOF window, then 2s after `start_kill`). `Drop` can't `.await`, so the sleep was the recommended sync-reap pattern, *but* `ClaudeSession` lives behind `Arc<tokio::sync::Mutex<…>>` and is dropped from inside async Tauri commands — meaning that 7s of `thread::sleep` ran on a tokio worker thread, stalling every other async task sharing that worker (the whole UI, plus any concurrent IPC command on the same runtime). The `Drop`-flavour twin of the blocking-on-worker anti-pattern the `spawn_blocking` command fix had just closed.
- **Consequences**: The reap now runs detached on the blocking pool. `child` became `Option<Child>` so `Drop` can `take()` ownership and hand it to a fire-and-forget `tokio::runtime::Handle::spawn_blocking` closure (along with the stderr-drain `JoinHandle`); dropping the returned `JoinHandle` *detaches* a `spawn_blocking` task (it still runs to completion — distinct from `Child::kill()`, whose future must be awaited to actually send the signal, the checkpoint-4 bug). The graceful-then-forceful sequence is preserved verbatim (close stdin → 5s EOF window → `start_kill` + 2s), so claude still gets a chance to flush its `--resume` session state before SIGKILL; `spawn_blocking` was chosen over `tokio::spawn` + `child.wait().await` specifically to keep that bounded reap rather than immediate-kill-and-detach. Ordering preserved: the stderr-drain task is aborted only *after* the child is reaped, so a verbose child can't fill its stderr pipe buffer and block on exit during the graceful window. A `Handle::try_current()` guard falls back to a synchronous `reap_child` when no runtime is attached (process teardown / drop from a non-runtime thread) — blocking is fine there (nothing async to stall) and it prevents a zombie. `reap_child` was extracted as a free function shared by both paths; `poll_reap` is unchanged. No behavior change observable to callers or the live integration tests (which assert on session semantics, not which thread reaps).

## 2026-07-10 — Resolve `claude`/`git` to absolute, vetted paths at spawn

- **Status**: accepted
- **Context**: Both subprocess spawns handed a bare name to the OS PATH search (`Command::new("claude")` / `Command::new("git")`). With a `.` or empty `$PATH` entry present, the bare-name search resolves against the process cwd — which is a user-selected project for `claude` (where the agent auth token is also injected into the child env) and a scanned repo for `git`. A project shipping a `claude`/`git` script could otherwise run that script under LoopDeck's privileges. This was flagged by the production-readiness audit as "over-broad capabilities + PATH-resolved binaries".
- **Consequences**: A new `binary` module resolves each name against `$PATH` *ourselves* before spawning: it skips every non-absolute `$PATH` component (closing the cwd-hijack vector entirely), vets the candidate is a regular executable file (Unix execute bit), and pins the result in a `OnceLock` so a later `$PATH` mutation can't redirect subsequent spawns. `claude_session.rs` and `git.rs` route through it. Failure semantics split by consumer — `claude` must exist to function (hard `AppError::Agent`), `git` is advisory metadata (soft `None` → callers see "no git info", matching the prior failed-spawn behavior). `git.rs` switched from `current_dir(repo_path)` + bare name to a resolved path + `git -C <repo_path>`. Test spawns (`git.rs` `#[cfg(test)]` setup, `agents.rs` `apply_agent_config` env-inspection tests) are deliberately left bare — they run in temp dirs the test owns or never spawn at all. The module documents two things it *cannot* prevent: a compromised `$PATH` where a legitimate earlier entry wins by design, and the GUI-launch minimal-PATH blind spot (no Homebrew/npm global dirs — discovering common install dirs is a separate follow-up). Closes the audit finding. See loops.md history entry of the same date.

## 2026-07-10 — Top-level React error boundary above `<App>`

- **Status**: accepted
- **Context**: A render-time crash anywhere in the app tree blanked the window with no recovery. The router's `errorComponent` (`router.tsx`) only catches errors thrown *inside* a route — `ThemeProvider`, `RouterProvider`, `Toaster`, and the root `App`'s own render all fall outside its reach and degrade to React's default uncaught-error blank screen. The P2 hardening list called for "a top-level React error boundary above `<App>`". React error boundaries are necessarily class components (`getDerivedStateFromError` + `componentDidCatch`), the only construct that intercepts render/lifecycle/constructor errors.
- **Consequences**: New `RootErrorBoundary` class component (`src/components/shared/RootErrorBoundary.tsx`) wraps `<App>` inside `React.StrictMode` in `main.tsx`. State is `{ error, resetKey }`. Recovery is split into two buttons with deliberately different guarantees: **Reload app** (`window.location.reload()`) is the guaranteed path — LoopDeck is offline-first so on-disk data (`.loopdeck/`, `config.yaml`, the keychain auth token) survives untouched; **Try again** is best-effort — it clears the boundary and remounts the subtree by bumping `resetKey` applied as a `Fragment` `key` (dropping transient React state; Zustand rehydrates from persistence), but a deterministic crash re-trips immediately. The fallback is intentionally self-contained (no Tauri/router/theme/store imports) so it renders even when the provider it wraps is what crashed — it relies only on the base OKLCH tokens in `:root` (`styles.css`), present without `ThemeProvider`. Styling mirrors the nearest sibling, the router's `ErrorComponent` (centered max-w-md card, identical primary/secondary button classes), and `cn()` was dropped for the single unconditional className to match that sibling's plain-string style. A collapsible error-details panel exposes `error.message` + stack for dev diagnostics. Scope is explicitly the render path only: async, event-handler, and `useEffect`-callback errors are not caught by React boundaries and continue to surface through the existing `appStore.error` banner + toasts. See loops.md history entry of the same date.

## 2026-07-19 — Atomic writes via temp-file + fsync + same-dir rename; last-known-good backup for the registry

- **Status**: accepted
- **Context**: Phase 2 of the trust-boundary hardening PRD required critical on-disk state (registry, project config, loops, decisions, PRDs, generated settings, transcripts) to survive crashes and malformed data. The pre-Phase-2 codebase used `std::fs::write` (truncate-then-write) everywhere, and the most dangerous instance was the registry: `GlobalConfig::load` on malformed YAML returned Err, `lib.rs` caught it with `unwrap_or_else(|e| { let fresh = default(); fresh.save(); fresh })` — silently overwriting the malformed primary with an empty default and wiping the user's project list. We considered (1) moving to SQLite with WAL mode, (2) pulling in the `tempfile` or `atomicwrites` crate, (3) hand-rolling a small `persist::atomic_write` helper using the temp + fsync + same-dir rename pattern the filesystem already provides, or (4) just fixing the silent-overwrite in lib.rs and leaving `std::fs::write` elsewhere.
- **Consequences**: Chose (3). SQLite was rejected as a non-goal (the PRD explicitly says "no database, no event-sourcing architecture") and would be a massive schema migration for state that's already YAML/JSON/Markdown. The `atomicwrites`/`tempfile` crates were rejected because the primitive is ~80 lines and full control over the temp+fsync+rename sequence matters (dropping the file handle before the rename for Windows compatibility, PID-keyed temp names so concurrent writers can't collide, cleaning up the temp on error). The helper sits in one new module (`persist.rs`) and is now used by every critical-write site. The registry additionally gets a `.bak` sibling written before every overwrite via `std::fs::copy`, and `load_from_path` runs a 4-step recovery (primary missing → default; primary parses → load; primary malformed → try backup and warn; both malformed → Err). `lib.rs` startup turned the silent-overwrite into a hard `error!` + `exit(1)` — the user gets a structured log line and the malformed file stays on disk for manual recovery, never silently wiped. Test-friendly inner fns (`load_from_path` / `save_to_path`) take explicit paths so the recovery tests don't need an env-var override or a production-code-path detour. Transcript appends stay append-mode (`OpenOptions::append`) with a `flush()` but no `fsync` — per-turn fsync would tank streaming throughput, and the PRD accepts line-atomic appends as sufficient. The `flush()` is enough to make an append visible to a same-process read without waiting for OS writeback. Tradeoff: every critical save now pays an fsync; in practice these are infrequent (registry updates, loop toggles, settings regen — not per-token) so the latency is invisible. See loops.md history entry of the same date.

## 2026-07-19 — ConfirmChanges as the default permission mode

- **Status**: accepted
- **Context**: Phase 1 of the trust-boundary hardening PRD exposed four places where the agent permission story was contradicting itself: (a) the Claude spawn flag ran `--permission-mode acceptEdits` (auto-approving Edit/Write/NotebookEdit inside Claude) while its own doc comment claimed `default`; (b) `skills.rs::setup_hooks` seeded `Edit(*)`, `Write(*)`, and broad build-runner Bash rules into the generated `.claude/settings.json`, short-circuiting LoopDeck's policy at the Claude layer; (c) `PermissionPolicy::allow_by_default()` and `PolicyDefault::Allow` names implied auto-approve-everything while the actual behavior was confirm-first (because `MANUAL_APPROVAL_TOOLS` interception in `answer_control_request` runs before the fallback `decide`); (d) the approval card had no effective-mode indicator. Net effect: all file edits + cargo/npm/go/pnpm/yarn command execution were silently auto-approved, and the UI implied more gating than existed. We considered (1) adding a new `AutonomousProject` mode as the "permissive" option and making `ConfirmChanges` the new strict default, (2) just fixing the spawn flag + allowlist without renaming the types, or (3) making the existing confirm-first behavior honest everywhere — flag, allowlist, type names, and UI — without adding new modes yet.
- **Consequences**: Chose (3). The four-arm `answer_control_request` flow (AskUserQuestion → destructive floor → `MANUAL_APPROVAL_TOOLS` interception → fallback policy) was already confirm-first by construction, so the work was making the other three layers match it rather than building a new mode. Concretely: spawn flag flipped to `default` (routes every un-ruled call through LoopDeck); broad `Edit(*)`/`Write(*)`/build-runner rules removed from the generated allowlist (read-only Bash rules kept); types renamed `PolicyDefault` → `PermissionMode` (`Allow` → `ConfirmChanges`), `allow_by_default` → `confirm_changes`; a `PermissionModeBadge` component surfaces the posture in both agent headers. `AutonomousProject` was *not* added as a type variant despite the initial plan suggesting it as future-proofing — under `ConfirmChanges` both match arms would return `Allow` (the gating happens upstream), so adding it now would be a dead match arm (YAGNI); it lands in Phase 3 alongside the path-containment helpers it actually needs. The `Deny` variant was kept against the plan's suggestion to delete it: the ignored `test_session_deny_path_is_graceful` integration test uses `PolicyDefault::Deny` to verify the CLI recovers from a hard deny without hanging, a real safety property worth keeping verified. Tradeoff accepted per the PRD: file edits the agent used to do silently now park on an approval card, causing more prompts — the "Always allow" button on the approval card is the documented escape hatch for users to add narrow rules (e.g. `Bash(npm run test:*)`) to `.claude/settings.local.json` once they trust a project. See loops.md history entry of the same date.

## 2026-07-19 — Transient gateway-error retry for agent turns

- **Status**: accepted
- **Context**: LoopDeck spawns the `claude` CLI subprocess and consumes its turns as NDJSON. A transient provider failure (Anthropic-compatible `529 overloaded`) does not crash the CLI or surface as a transport error — it arrives as a normal `Ok(AgentResponse { is_error: true, result: "API Error: 529 … overloaded …" })`. Without retry, one transient blip ended the loop run and forced a manual re-prompt. We considered (a) retrying at the HTTP layer inside a hypothetical direct-gateway client, (b) surfacing every `is_error` to the user with no retry, or (c) retrying in LoopDeck based on the error *text*.
- **Consequences**: Chose (c) — a new `retry` module decides eligibility by substring match on the result text (`529` or `overloaded`, case-insensitive), because LoopDeck does not call the gateway directly and there is no structured status code available on this side of the `claude` CLI. Backoff is exponential with a cap (`MAX_ATTEMPTS = 4`, base 2s, factor 2, cap 30s → ~2s/4s/8s then give up). Non-transient errors (401, 400, "not logged in") return immediately — retrying them only delays user feedback. Two wrappers (`send_with_retry`, `send_streaming_with_retry`) sit between the pipeline helpers and `ClaudeSession::send_message[_streaming]`; transcript recording stays *outside* the wrappers so a retried turn is recorded as a single exchange, not N. The streaming wrapper emits a new `ClaudeEvent::Retrying { attempt, max_attempts, backoff_ms, error }` between attempts so the UI can show "Retrying 2/4 in 4s…" instead of a silent double-`Result`; the final `Result` remains authoritative. The substring match is deliberately loose on wording (`overloaded`) and tight on status (`529`) so it survives minor CLI phrasing changes without matching unrelated errors; the test suite pins both positive and negative strings, including the real captured 529 message. Retry against a live overloaded gateway is not unit-testable from this side of the CLI, so the contract is anchored by the substring + backoff tests. See loops.md history entry of the same date.

## 2026-07-10 — Lenient coercion for malformed user hook events; logged exit for unrecoverable Tauri startup

- **Status**: accepted
- **Context**: Two `expect()` sites surfaced in the `panic = "abort"` audit. (1) `skills.rs::find_or_create_matcher_group` called `.expect("hook event must be an array")`; reachable from `setup_hooks` on the import path, a user's pre-existing `.claude/settings.json` with a non-array hook event value (`"Stop": "not-an-array"`) would abort the whole process — the existing guard only created the array when the key was *missing*, not when it was present-but-wrong-typed, so a malformed value flowed straight into the panic. (2) `lib.rs` `.run(tauri::generate_context!()).expect("error while running tauri application")` — the canonical Tauri startup pattern, but under abort a startup failure (WebView/context/plugin init) is a raw panic with no structured record for the user to find.
- **Consequences**: (1) `find_or_create_matcher_group` now coerces a non-array hook event to `[]` and proceeds, mirroring the lenient JSON-parse→`{}` fallback already at the top of `setup_hooks` — malformed user config is treated as "no existing hooks," not a fatal import error (erroring the whole import on one odd hook field would be worse than overwriting it). The surviving `.expect()` is now provably unreachable (coerced immediately above) — an invariant, not a user-input abort. (2) `lib.rs` switched to `if let Err(e) = ...run(...) { tracing::error!(...); std::process::exit(1); }`: same termination outcome, but a structured log line (tracing initialized at the top of `run()`) replaces the raw panic, and the panic/abort source is removed. `std::process::exit` skips destructors, but so does an abort, and nothing meaningful is live at that point. The three remaining production `.unwrap()`/`.expect()` sites (`permission.rs` `split_stages` stages-non-empty ×4, `commands.rs:1018` singleton-vec, `memory.rs:174` re-derived checklist prefix) were classified as provable programmer invariants and left untouched — the audit's concern under abort is panics reachable from bad input, not every unwrap, and converting invariants to defensive `match`es would be churn with no robustness gain. See loops.md history entry of the same date.

