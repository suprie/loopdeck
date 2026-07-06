# Loops

## Current

- **Started**: 2026-07-05
- **Goal**: Make LoopDeck production-ready. Phase 1 (security stop-the-bleeding)
  is done; Phases 2-6 remain — distribution, hardening, quality gates, docs/policy,
  and UX polish. Audit was a three-pronged review (Rust backend, React frontend,
  ops/tooling) that produced the action items below.
- **Status**: in_progress

## Next Steps

### P2 — Distribution (signing / notarization / updater)
- [ ] Configure macOS signing identity in `tauri.conf.json` (`bundle.macOS.signingIdentity`) + Windows `certificateThumbprint`/`tsp` server; feed via CI secrets
- [ ] Notarization: wire `APPLE_ID` / `APPLE_PASSWORD` / team ID into the macOS release pipeline
- [ ] Updater: add `tauri-plugin-updater` config (`pubkey` + `endpoints`) and a `TAURI_SIGNING_PRIVATE_KEY`-based release workflow
- [ ] Release CI: `tauri-apps/tauri-action` workflow producing signed `.dmg` / `.msi` / `.AppImage` + signed `latest.json`
- [ ] Bundle metadata: fill in `bundle.publisher`, `bundle.category`, `bundle.copyright`, `bundle.shortDescription`

### P3 — Hardening (correctness, robustness, secret hygiene)
- [ ] Move auth token out of plaintext `~/.config/loopdeck/config.yaml` into the OS keychain (macOS Keychain / Windows Credential Manager / Secret Service); `chmod 600` is the interim floor
- [ ] Wrap blocking I/O in `spawn_blocking`: `list_projects`, `rescan_project`, `scan_directory`, `import_project` (`commands.rs`) — they currently run sync walkdir + git subprocess spawning inside `async` Tauri commands
- [ ] Fix `Drop` blocking: `claude_session.rs:1183-1194` sleeps up to 7s reaping the child on a tokio worker thread — move to `spawn_blocking` or kill+detach with tokio `wait`
- [ ] Resolve `claude` and `git` to absolute, vetted paths at spawn (`claude_session.rs:202`, `git.rs:68,91,114,144,162`) to defeat PATH hijack
- [ ] Add a top-level React error boundary above `<App>` in `main.tsx` — pre-router crashes currently blank-screen with no recovery
- [ ] Audit `expect()`/`unwrap()` under `panic = "abort"`; in particular `skills.rs:362` (a malformed user `settings.json` aborts the process on import) and `lib.rs:77`
- [ ] Add an absolute per-turn deadline or parked-slot expiry (`claude_session.rs`) — parked turns currently hold the per-project lock indefinitely
- [ ] Cap unbounded accumulation in `ResponseAccumulator` (`agents.rs:603-625`) — abort past a block/byte limit
- [ ] Cap log retention in `logging.rs` (daily rolling appender grows forever); confirm no `auth_token` is ever logged
- [ ] Replace `eprintln!` diagnostics in `project.rs:43,76,172` with `tracing::debug!`
- [ ] Strengthen `check_destructive_floor` further: prefix deny-list is now argv-analyzed, but `mv`/`cp` targeting `/`, `/etc`, `/usr`, `$HOME` root are still best-effort
- [ ] Reconcile the `claude_session.rs:218-224` doc comment ("default") with the actual `--permission-mode acceptEdits` arg

### P4 — Quality gates (CI, lint, tests)
- [ ] CI: `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `npm ci`, `tsc --noEmit`, `npm run build` on macOS/Linux/Windows matrices
- [ ] Frontend lint/format: ESLint + @typescript-eslint + eslint-plugin-react-hooks + Prettier; add `npm run lint`
- [ ] Rust lint/format: `rustfmt.toml` + `clippy.toml`; enforce `-D warnings`
- [ ] Frontend tests: vitest + @testing-library/react — prioritize `streamingState`, `groupLoopRuns`, the IPC wire-conversion in `tauri.ts:216-235`, and the streaming-turn lifecycle
- [ ] E2E: `@tauri-apps/driver` + WebdriverIO smoke covering scan → import → edit
- [ ] Dependabot + `cargo audit` + `npm audit` in CI; pin toolchain (`engines`, `packageManager`, `rust-toolchain.toml`)
- [ ] Type-narrow Tauri IPC at the boundary (zod/valibot); assert-never on the `ClaudeEvent.type` switch
- [ ] Pre-commit hook wiring the lint/format checks above
- [ ] SBOM / license check (`cargo bundle-licenses`) in CI

### P5 — Docs & policy
- [ ] Add `LICENSE` (MIT) + `license` field in `package.json` and `Cargo.toml`
- [ ] Add `SECURITY.md` documenting the agent threat model (subprocess spawn, `acceptEdits`, allow-by-default + destructive floor) and vuln-reporting policy
- [ ] Add `CONTRIBUTING.md` (dev setup, commit style, branch model, PR checklist, `.loopdeck/` memory convention)
- [ ] Refresh README/CLAUDE — they claim "no agents" but the code has a full agent runtime with 29 IPC commands; fix the stale "30 tests" count (now 188)
- [ ] `git rm --cached .DS_Store .claude -r` (both tracked despite being in `.gitignore`)
- [ ] Move `docs/researchs/*.json` (~220KB captured provider payloads) to LFS or an external artifact store
- [ ] Add `.env.example` (DONE in Phase 1) — keep in sync as new env vars are introduced
- [ ] Architecture doc + user install guide: `docs/architecture.md`, `docs/USER_GUIDE.md`
- [ ] `CHANGELOG.md` (or generate via release tooling)

### P6 — UX polish (a11y, i18n, perf)
- [ ] A11y: replace the hand-rolled history dropdown with Radix `Popover`/`DropdownMenu` (`AgentPanel.tsx:852-905`) — gives focus trap, Esc, roving focus, ARIA
- [ ] A11y: give `AskUserQuestionCard`/`PermissionApprovalCard` real `role="radio"`/`role="checkbox"` + keyboard nav (`Chat.tsx:622-879`); currently glyphs (`●`/`○`) are the only state indicator
- [ ] A11y: `aria-live="polite"` on the streaming region; `aria-label` on the send button
- [ ] A11y: visually-hidden native `<input type="checkbox">` for GFM task-list items (`Markdown.tsx:73-87`)
- [ ] i18n scaffolding (react-intl/i18next) — every user-facing string is currently a hardcoded English literal
- [ ] Chat perf: `useMemo` `groupLoopRuns`, memoize completed `TurnBubble`, virtualize the transcript (`@tanstack/react-virtual`)
- [ ] Throttle `streamingState` delta writes (currently O(blocks) per token; use `useSyncExternalStore` selector + rAF)
- [ ] Auto-scroll: only stick-to-bottom when the user is already near the bottom (`Chat.tsx:921-925`)
- [ ] Persist only `selectedProject.path` in Zustand (not the full `ProjectEntry`) to avoid schema drift
- [ ] Move module-level mutable `lastSelectedPath` (`AgentRunner.tsx:30`) into the Zustand store
- [ ] Wire or remove the dead `cmdk` dependency; remove the misleading `⌘K` hint (`AppShell.tsx:152`)
- [ ] Fetch model presets from backend/config instead of hardcoding model IDs (`Settings.tsx:24-29`)
- [ ] Verify `lucide-react ^1.21.0` is the intended major (current major is much higher — likely a typo)
- [ ] Consolidate the three divergent relative-time formatters (`lib/time.ts`, `AgentPanel.tsx:53-70`, `Chat.tsx:96-102`)
- [ ] Add source-map strategy for prod error reporting; inject `__APP_VERSION__` via Vite `define`

## Parking Lot
- [ ] **Move agent control into LoopDeck app** — When LoopDeck can spawn/manage AI agents from within the app (not just the terminal), it should own all agent configuration: CLAUDE.md, skills, hooks, and memory conventions. The current `.claude/settings.local.json` hooks (PreToolUse dirty flag, Stop hook reminder) are temporary workarounds that only work in the Claude Code terminal context. Once LoopDeck controls the agent runtime, it can intelligently decide when to prompt for memory updates, apply project-specific instructions, and manage skills — without relying on external hook files.
- [ ] **macOS App Sandbox** — enable App Sandbox + scoped entitlements (user-selected files only) so a misbehaving agent is bounded by more than the OS user. Requires rethinking `current_dir(project_path)` access patterns.

## History

### 2026-07-05 — Phase 1: Stop the bleeding (security)

- **Status**: completed
- **Completed**: 2026-07-05

Closed the critical/high security findings from the production-readiness audit.
Reframing documented for future readers: `permission.rs` already parks
`Bash`/`Edit`/`Write`/`NotebookEdit`/`WebFetch`/MCP on a manual-approval card
(`MANUAL_APPROVAL_TOOLS`), so the user *is* in the loop for mutating tools; the
"allow-by-default" only governs read-only tools. The destructive floor is the
backstop for when a user clicks "Allow" without reading — that's what got
strengthened, rather than flipping the whole posture.

**Changes.**

- **Leaked API key removed from source.** `sk-64a72…` was hardcoded in
  `claude_session.rs` test_config and a dead commented block in `agents.rs`.
  `test_config()` now reads `LOOPDECK_TEST_AUTH_TOKEN` from env (also
  `LOOPDECK_TEST_BASE_URL`/`LOOPDECK_TEST_MODEL`); the dead block was deleted.
  History scrub intentionally skipped (rotated literal is dead). New
  `.env.example` documents all runtime + test env vars.
- **Strict CSP** (`tauri.conf.json`). Was `null`; now `default-src 'self' ipc:
  http://ipc.localhost`, `script-src 'self'` (no unsafe-inline/eval), only
  `style-src 'unsafe-inline'` (Tailwind v4 + Radix need it). img/font/data
  allowlisted minimally.
- **Shell injection fixed** (`commands.rs`). New `resolve_dir_arg()` canonicalizes
  and rejects non-directories — used by both `open_in_finder` and
  `open_in_terminal` (blocks the macOS `open "x-apple-…"` handler trick).
  macOS terminal launchers no longer interpolate the path into AppleScript —
  it's passed as a separate `osascript … <path>` argv with `on run argv` +
  `quoted form of`. Windows dropped `cmd /C "cd /d {path}"` for `cmd /K` +
  `current_dir()`. Linux was already safe.
- **Destructive floor strengthened** (`permission.rs`). Was a 5-rule prefix list
  trivially bypassed (`rm -r -f`, `find -delete`, `ls; rm -rf`, etc.). Now: lex
  with `shell-words`, quote-aware stage split on `| ; && ||`, walk every stage,
  peel privilege wrappers (`sudo`/`command`/`exec`/`eval`/…), inspect argv[0] +
  flags (case-insensitive, short-flag bundling aware). Catches `rm` with
  recursive+force in any order/form, `find -delete`/`-exec rm`, `dd`, `mkfs*`,
  `chmod/chown -R`, all force-push variants including `--force-with-lease`,
  `curl|sh` pipe-to-shell, smuggled pipeline stages, and wrappers. Falls back
  to legacy prefix rules if parse fails. +13 new tests, all 30 floor tests pass.
- **Capabilities scoped** (`capabilities/default.json`). `shell:default` →
  `shell:allow-open` (the only shell function the frontend uses is `open()` in
  `Markdown.tsx`).
- **Markdown href allowlist** (`Markdown.tsx`). The `a` override handed any
  `href` to Tauri `open()`; now only `http:`/`https:`/`mailto:` get through,
  others drop `href` entirely. Belt-and-braces alongside the CSP.

**Verification.** `cargo test --lib` 188 passed / 0 failed / 7 ignored (was 175
before; +13 new floor tests). `cargo clippy --all-targets` 0 warnings. `cargo fmt`
applied. `npm run build` passes (tsc + vite).

**Outstanding from this loop (carried to P2-P6).** Code signing / notarization /
updater (P2); plaintext token → keychain, `spawn_blocking`, `Drop` blocking,
absolute-path binary resolution, top-level error boundary (P3). CI, lint, frontend
tests (P4). LICENSE / SECURITY.md / CONTRIBUTING / README refresh (P5).

Files changed: src-tauri/src/{permission.rs, commands.rs, claude_session.rs,
agents.rs, Cargo.toml}, src-tauri/{tauri.conf.json, capabilities/default.json},
src/components/shared/Markdown.tsx, .env.example (new), .loopdeck/loops.md.

### 2026-07-05 — Production readiness audit

- **Status**: completed
- **Completed**: 2026-07-05

Three-pronged read of the codebase (Rust backend, React frontend, ops/tooling)
to enumerate what's needed before production. Verdict: not production-ready —
solid foundation (typed IPC, good Rust test coverage, navigation-stable
streaming stores) but real blockers in security, distribution, and quality
gates. Findings fed directly into the Phase 1-6 next-steps list above.

Top blockers surfaced: leaked API key in git history; no CSP; shell injection
in `open_in_terminal`; over-broad capabilities + PATH-resolved binaries; no
code signing / notarization / updater; no CI; no LICENSE/SECURITY/CONTRIBUTING;
zero frontend tests; no markdown sanitization; A11y failures on hand-rolled
dropdowns and question/approval cards.

### 2026-07-03 — TanStack Router (replace Zustand view switching)

- **Status**: completed
- **Completed**: 2026-07-03

Replaced the Zustand `currentView` + conditional rendering pattern with
`@tanstack/react-router` v1 using memory history (no browser URL bar in Tauri).
Views are now proper routes with type-safe navigation.

**`router.tsx`** (NEW — `src/router.tsx`, 200 lines):
- `AppShellLayout` — root route component with sidebar nav, error banner, and
  `<Outlet />` for child route content. Nav items use `<Link>` components;
  active-state detection uses `useRouterState().location.pathname`.
- Route tree: `/` (Dashboard), `/activity`, `/agent`, `/decisions`, `/loops`,
  `/settings`, `/import`, `/project/$projectPath` (URL-encoded filesystem path).
- `createMemoryHistory({ initialEntries: ["/"] })` — in-memory routing for
  the Tauri desktop environment.
- Re-exports `RouterProvider`, `Outlet`, `useNavigate`, `useParams`, `Link`
  for convenience.

**Store changes** (`appStore.ts`):
- Removed `currentView` and `setCurrentView` from the Zustand store — navigation
  is now the router's responsibility.
- `setSelectedProject` simplified: no longer sets `currentView` to `"detail"`.
- Persisted state (`loopdeck-nav`) reduced to `selectedProject` + `detailTab`.

**Hook changes** (`useProjects.ts`):
- `scanFolder` → navigates to `/import` after scan.
- `importRepo` → navigates to `/` after import.
- `removeProject` → navigates to `/` after removal.
- All use `useNavigate()` instead of the old `setCurrentView`.

**View component changes:**
- `Dashboard.tsx`: `handleSelect` and `handleStart` now call `navigate({ to: "/project/$projectPath", params: { projectPath: encodeURIComponent(path) } })` instead of relying on `setSelectedProject` to change views.
- `ProjectDetail.tsx`: reads `projectPath` from `useParams()`, decodes with `decodeURIComponent`, syncs `selectedProject` via `useEffect`. Back button uses `navigate({ to: "/" })`.
- `ImportFlow.tsx`: back button uses `navigate({ to: "/" })`.
- `AppShell.tsx`: reduced to only `PageHeader` export (the layout moved to `router.tsx`'s `AppShellLayout`).

**Design decisions:**
- **Memory history over browser history** — Tauri has no URL bar, so
  `createMemoryHistory` is the natural fit. Routes are purely internal.
- **URL-encoded project paths** — filesystem paths like `/Users/foo/bar` are
  URI-encoded so they travel as a single route segment (`/project/$projectPath`).
  `encodeURIComponent`/`decodeURIComponent` at the navigation/cosumption boundary.
- **Zustand for data, Router for navigation** — the store still owns
  `selectedProject`, `detailTab`, `pendingAgentStart`, and the project list.
  The router owns the current location. No more store-driven view switching.
- **Persisted selection reconciliation** — `loadProjects` still resolves the
  persisted `selectedProject` against the fresh project list on startup,
  clearing it if the project was removed or refreshing it if stale.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors.

Closes loops.md #13 (TanStack Router).

Files changed: src/{router.tsx (new), App.tsx, store/appStore.ts,
hooks/useProjects.ts, components/{layout/AppShell.tsx, dashboard/Dashboard.tsx,
detail/ProjectDetail.tsx, import/ImportFlow.tsx}},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Standalone Decisions + Loops views

- **Status**: completed
- **Completed**: 2026-07-03

Added two standalone top-level views — Decisions and Loops — that aggregate data
across all projects. Previously this content was only accessible inside
`ProjectDetail`'s tabbed sidebar; now both are first-class views accessible from
the main navigation.

**`DecisionsView.tsx`** (NEW — `src/components/decisions/DecisionsView.tsx`, 240 lines):
- Loads all decisions from every project's `.loopdeck/decisions.md`.
- Filters: free-text search (title, context, consequences, project name),
  status tabs (All / accepted / proposed / superseded), project dropdown.
- Cards grouped by month, each showing project name, date, title, status badge,
  context preview (2-line clamp). Click to expand full context + consequences.
- Colour-coded status: accepted=green, proposed=amber, superseded=muted.
- States: loading spinner, error, empty ("No decisions yet"), filtered-empty
  with "Clear filters" link.

**`LoopsView.tsx`** (NEW — `src/components/loops/LoopsView.tsx`, 210 lines):
- Loads all loop statuses from every project's `.loopdeck/loops.md`.
- Stats bar: active loops, pending next steps, completed history, project count.
- Cards sorted: in-progress loops first, then by project name. Active cards
  have a green-tinted border + background highlight, Play icon, and "Active" tag.
  Completed/paused get CheckCircle/Circle icons.
- Expanded card shows: current loop metadata (started/completed dates), next
  steps list (checked items in green strikethrough, unchecked with arrow icon),
  history summary (last 5 completed loops with "+N more" overflow).
- States: loading, error, empty ("No loops yet").

**Integration:**
- `types/index.ts`: `AppView` gained `"decisions"` and `"loops"`.
- `AppShell.tsx`: Lightbulb and Repeat nav items added.
- `App.tsx`: Both views wired.

**Design decisions:**
- **Project-level sorting for Loops** — active projects first, then
  alphabetical. The user's primary question is "what should I work on next?",
  so active loops must surface above completed ones. Within the same
  activity tier, alphabetical is predictable.
- **Expand-in-place cards vs separate pages** — both views use click-to-
  expand cards instead of navigating to a detail page. Since there's no
  TanStack Router yet, per-card routing would need Zustand state management
  (selected decision ID, selected loop path), which adds complexity without
  clear benefit. The expand-in-place pattern shows detail inline while keeping
  the list visible for context.
- **Month grouping for Decisions** — decisions use month-level buckets
  (not day-level like Activity Feed) because decision dates are date-only
  strings (no time component) and there are typically fewer decisions than
  agent turns. Month grouping avoids single-item day sections.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors.

Closes loops.md #11 (standalone Decisions) and #12 (standalone Loops).

Files changed: src/{types/index.ts, App.tsx,
components/{layout/AppShell.tsx, decisions/DecisionsView.tsx (new),
loops/LoopsView.tsx (new)}}, .loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Activity Feed view (`/activity`)

- **Status**: completed
- **Completed**: 2026-07-03

Added a chronological event feed that aggregates activity from all registered
projects into a single timeline — agent turns, architectural decisions, and
development loop completions, merged and sorted by timestamp.

**`ActivityFeed.tsx`** (NEW — `src/components/activity/ActivityFeed.tsx`, 240 lines):

- **Data collection**: on mount, iterates all registered projects and fetches
  three data sources per project: `agentGetConversation` (turns),
  `getDecisions` (decisions), and `getLoops` (loop history). Each source is
  caught independently — a missing file (e.g. no transcript yet) is skipped
  silently without failing the entire feed.
- **Event unification**: three collectors (`turnsToEvents`, `decisionsToEvents`,
  `loopsToEvents`) normalise each source into a common `ActivityEvent` shape
  with `kind` discriminator (`turn_user` | `turn_assistant` | `turn_error` |
  `decision` | `loop_completed`), project name/path, timestamp, one-line
  summary, and optional detail body.
- **Timeline rendering**: events sorted descending by timestamp, grouped into
  date buckets (Today / Yesterday / "Monday, July 1"), each group shown as a
  section with a sticky date heading, count badge, and a divider line.
- **Event row**: colour-coded icon per kind (User=neutral, Bot=primary,
  Error=destructive, Decision=warning, Loop=success). Project name + timestamp
  on the top row, summary on the second (line-clamped to 2 lines). Click-to-
  expand detail for events with body content (turn text, decision context,
  loop metadata) shown in a monospace block with max-height + scroll.
- **States**: loading spinner, error display, empty state ("No activity yet"
  with Activity icon and guidance text).

**Integration:**
- `types/index.ts`: `AppView` gained `"activity"`.
- `AppShell.tsx`: Activity icon nav item between Dashboard and Agent Runner.
- `App.tsx`: `ActivityFeed` rendered on `currentView === "activity"`.

**Design decisions:**
- **Unified event shape over separate sections** — merging conversations,
  decisions, and loops into a single `ActivityEvent[]` (discriminated by
  `kind`) keeps the timeline rendering logic simple: one sort, one grouping
  pass, one render loop. The alternative (separate sections per source) would
  fragment the chronology and require the user to mentally merge timelines.
- **Best-effort per-source fetching** — each API call per project is tried
  independently; a missing `.loopdeck/sessions/active.jsonl` doesn't prevent
  decisions and loops from appearing. This is important for projects that
  haven't run an agent yet but have decisions/loops from manual edits.
- **Date-only timestamps for decisions/loops** — decisions use `"2026-06-22"`
  format (no time component). We synthesise midnight UTC timestamps
  (`dateToTs`) so they sort correctly within the day's bucket. This means
  decisions always appear at the top of their date group, which is acceptable
  since they're typically batched by session, not by exact hour.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors.

Closes loops.md #10 (Activity Feed view).

Files changed: src/{types/index.ts, App.tsx,
components/{layout/AppShell.tsx, activity/ActivityFeed.tsx (new)}},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Agent Runner view (`/agent`)

- **Status**: completed
- **Completed**: 2026-07-03

Added a standalone, terminal-themed AI agent runner view accessible from the
sidebar. Unlike the project-specific Agent tab in ProjectDetail, the Agent
Runner is a top-level view where the user can switch between projects without
leaving the agent interface.

**`AgentRunner.tsx`** (NEW — `src/components/agent/AgentRunner.tsx`, 370 lines):

- **Left panel** (280px) — scrollable project list showing all registered
  projects with session metadata: live status indicator (green dot for active
  sessions within 30 min), turn count, last activity timestamp, and project
  path. Sorted: live sessions first, then by recency. Empty state when no
  projects are registered.
- **Right panel** — terminal-themed agent interface:
  - Header bar with terminal icon, `loopdeck agent ~/project-name` path, and
    project filesystem path.
  - Toolbar: **Run next loop** (green, starts/continues development loop from
    `.loopdeck/loops.md`) and **Reset** (archive transcript, drop process).
    Turn counter on the right.
  - Scrollable output area with monospace font (`JetBrains Mono`):
    - `❯ user` / `❯ assistant` prompts in green/primary with timestamps and
      token-usage stats
    - Turn content with `whitespace-pre-wrap` preserving agent formatting
    - Empty state: "Select a project" or "No conversation yet" with CTA
  - Live streaming: tool-call activity (diamond `◈` markers in warning color),
    streaming text with green pulsing cursor, busy indicator
  - Composer: green `❯` prompt + single-line input, Enter to send
- **Streaming orchestration**: reuses the same `Channel<ClaudeEvent>` pattern
  as `AgentPanel` — `runStreamingTurn(prompt?)` creates a Channel, wires
  `onmessage` for delta accumulation (`text_delta`, `tool_use`, `result`), and
  calls `agentStartLoopStreaming` / `agentSendMessageStreaming` accordingly.
  Transcript reload on Result event. Mounted guard for channel safety.
- **Session scanning**: on mount, queries all projects for their transcripts
  to derive live/not-live status (active if last assistant turn < 30 min ago).

**Integration:**
- `types/index.ts`: `AppView` gained `"agent"`.
- `AppShell.tsx`: Terminal icon nav item between Dashboard and Import Repo.
- `App.tsx`: `AgentRunner` component rendered on `currentView === "agent"`.

**Design decisions:**
- **Terminal theme not Chat bubble theme** — The Agent Runner uses monospace
  font, dark background (`oklch(0.13_0.01_270)`), prompt indicators (`❯`),
  and flat line-by-line output instead of rounded chat bubbles. This
  distinguishes it from the project-specific Agent Panel and signals
  "developer tool" rather than "chat app".
- **Standalone project selector** — Unlike AgentPanel (reached via Dashboard →
  project → Agent tab), the Agent Runner has its own project list, so the user
  can switch between agent sessions without navigating back to the dashboard.
  This is the "tmux for AI agents" pattern.
- **Shared streaming pattern** — `AgentRunner` does NOT reuse the `Chat`
  component because the terminal aesthetic requires fundamentally different
  rendering (no avatars, no bubbles, prompt-based layout, monospace font). It
  DOES reuse the same streaming orchestration pattern: Channel → `onmessage` →
  delta accumulation → Result → reload. The two components (AgentPanel+Chat,
  AgentRunner) are rendering-siblings but orchestration-cousins — same
  approach, different visual output.

**Verification.** `tsc --noEmit` clean; `cargo check` 0 new errors (3
pre-existing warnings).

Closes loops.md #9 (Agent Runner view).

Files changed: src/{types/index.ts, App.tsx,
components/{layout/AppShell.tsx, agent/AgentRunner.tsx (new)}},
.loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Streaming frontend chat UI with Tauri Channel
- **Status**: completed
- **Completed**: 2026-07-03

Upgraded `AgentPanel.tsx` from batch-only APIs to real-time streaming via Tauri
`Channel<ClaudeEvent>`. The agent's response now renders token-by-token as it
arrives instead of showing a spinner for the full turn duration.

**Backend.**
- `commands.rs`: new `agent_start_loop_streaming` command — builds the next-loop
  prompt server-side (same `build_next_loop_prompt`) and sends via
  `send_and_record_streaming` so the Start-next-loop flow also streams.
- `lib.rs`: registered `agent_start_loop_streaming` in the invoke handler.

**Frontend.**
- `lib/tauri.ts`: `agentStartLoopStreaming(path, onEvent: Channel<ClaudeEvent>)`
  — typed IPC wrapper for the new streaming start-loop command.
- `AgentPanel.tsx`: full rewrite with streaming architecture:
  - `runStreamingTurn(prompt?)` — shared helper that creates a `Channel`,
    wires `onmessage` to accumulate `TextDelta`/`ThinkingDelta`/`Result` events,
    calls the appropriate streaming IPC, and reloads the transcript on completion.
  - Both "Start next loop" and free-form Send use streaming — no batch fallback.
  - `mountedRef` guards channel event handlers against post-unmount state updates.
- `Chat.tsx` (NEW, extracted from AgentPanel) — reusable presentational component:
  - `TurnBubble` — persisted transcript turn (user/assistant/error with usage meta).
  - `ThinkingBlock` — collapsible model reasoning (ChevronDown/ChevronRight toggle).
  - `StreamingBubble` — live token accumulation with typewriter cursor, spinner→Bot
    avatar transition on Result, usage/duration meta in transient window.
  - `Chat` container — scrollable transcript, auto-scroll, empty state, error banner
    with dismiss, composer (Enter to send, Shift+Enter for newline).
  - Pure presentational — no Tauri/Channel imports; all state via props.

**Design decisions:**
- Channel events are the single source of truth for turn state. The invoke
  Promise is only used for infra-level error catching (timeout, no config,
  spawn failure). Model-level errors (`is_error: true`) are surfaced from the
  `Result` event, not from Promise rejection — consistent with the existing
  decision that streaming commands return `()` not `AgentResponse`.
- The `StreamingBubble` renders alongside persisted `TurnBubble`s during a
  turn. When the `Result` event triggers a transcript reload, the streaming
  bubble is naturally replaced by the newly-persisted turn. No imperative
  DOM manipulation — React reconciliation handles the transition.
- Thinking is hidden by default (collapsed) and toggled explicitly. This
  keeps the UI clean during normal turns while making extended thinking
  inspectable when needed.
- **Presentational Chat, streaming orchestration in AgentPanel** — `Chat`
  is a pure rendering component with zero Tauri knowledge, making it reusable
  (e.g. for a standalone `/agent` view) and testable in isolation. `AgentPanel`
  remains the single owner of Channel lifecycle and transcript persistence.
  State flows one way: Channel events → AgentPanel setState → Chat props.

**Verification.** `cargo check` 0 new errors (3 pre-existing warnings);
`cargo test --lib` 108/108 passed (5 ignored live); `tsc --noEmit` clean.

Closes loops.md "Frontend chat UI" step.

Files changed: src-tauri/src/{commands.rs, lib.rs}, src/{lib/tauri.ts,
components/detail/{AgentPanel.tsx, Chat.tsx (new)}}, .loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Streaming `send_message_streaming` with `ClaudeEvent` + `Channel<T>`
- **Status**: completed
- **Completed**: 2026-07-03

Added a streaming variant of `send_message` so the frontend can render assistant
tokens as they arrive instead of waiting for the full turn to buffer. The
streaming path shares the same process/lock/transcript pipeline as the batch
path — only the read loop differs.

**Backend.**
- `agents.rs`: new `ClaudeEvent` enum (Serialize, Clone) with three variants —
  `TextDelta` (one per `ContentBlock::Text`), `ThinkingDelta`, and `Result`
  (terminal; carries the full aggregated `AgentResponse` fields). Made
  `ContentBlock`, `AssistantMessage`, and `RawUsage` `pub(crate)` so
  `claude_session.rs` can iterate content blocks for per-block event emission.
- `claude_session.rs`: `send_message_streaming(&mut self, text, channel:
  &Channel<ClaudeEvent>)` — writes the user turn to stdin (same as `send_message`),
  then reads NDJSON lines, emitting per-content-block `ClaudeEvent`s as each
  `assistant` message arrives, accumulating into `ResponseAccumulator`, and
  emitting the terminal `ClaudeEvent::Result` on turn completion. Channel sends
  are best-effort (`let _ = channel.send(…)`) — a closed channel (frontend
  navigates away) is silently dropped. Same 180s timeout as `send_message`.
- `commands.rs`: `agent_send_message_streaming(path, prompt, on_event:
  Channel<ClaudeEvent>)` Tauri command + `send_and_record_streaming` helper
  (same pre-send user-turn append + post-send assistant-turn append as
  `send_and_record`, but calls `send_message_streaming`). Returns `()`
  rather than `AgentResponse` — the terminal `ClaudeEvent::Result` is the
  single source of truth for the frontend.
- `lib.rs`: registered `agent_send_message_streaming` in the invoke handler.

**Frontend.**
- `types/index.ts`: `ClaudeEvent` discriminated union type mirroring the Rust
  enum (`text_delta`, `thinking_delta`, `result`).
- `lib/tauri.ts`: `agentSendMessageStreaming(path, prompt, onEvent:
  Channel<ClaudeEvent>)` — typed IPC wrapper that passes the Tauri `Channel`
  from `@tauri-apps/api/core`.

**Design decisions:**
- The streaming and batch paths share `ResponseAccumulator` — they can't drift
  in how they aggregate the final response.
- Per-content-block emission (not per-NDJSON-line) matches the UI's natural
  rendering granularity: each `TextDelta` is a complete text fragment.
- `send_message` is *not* refactored to call `send_message_streaming` with a
  dropped channel — the batch path is simpler and doesn't allocate channel
  overhead; the duplication (~15 lines of stdin write) is small.
- Channel dropping is expected and non-fatal: the turn completes regardless,
  and the transcript is always recorded.

**Verification.** `cargo check` 0 new errors (3 pre-existing warnings);
`cargo test --lib` 108/108 passed (4 ignored live); `tsc --noEmit` clean.

Closes loops.md #7 (streaming variant).

Files changed: src-tauri/src/{agents.rs, claude_session.rs, commands.rs,
lib.rs}, src/{types/index.ts, lib/tauri.ts}, .loopdeck/{loops.md, decisions.md}.

### 2026-07-03 — Multi-project Claude session orchestration (Start button + Agent tab)
- **Status**: completed
- **Completed**: 2026-07-03

Wired the existing `ClaudeSession` into the LoopDeck UI — pressing **Start** on
a project card now spawns the agent, prompts it for the next loop from
`.loopdeck/loops.md`, and drives work via the orchestrator skill conventions.
Adds true cross-project parallelism, a persisted conversation transcript, and
resume-across-restarts.

**Phase 0 — resume spike (the gate).** The PRD's central risk —
`--resume <id>` composed with `--input-format stream-json` had never been
tested together — was de-risked first with a single ignored integration test
(`test_session_resume_after_restart`): plant a codeword → drop the session
(process dies, simulating restart) → re-spawn with `--resume <id>` → assert
recall. It passed against the live provider, so the full resume path was
built on top of it.

**Backend.**
- `claude_session.rs`: `spawn` gained `resume_session_id: Option<&str>`;
  pushes `--resume <id>` when set.
- `conversation.rs` (NEW): `ConversationTurn` (Serialize/Deserialize) +
  `load_conversation` / `last_session_id` / `append_turn` / `archive_conversation`.
  Storage: `<project>/.loopdeck/sessions/active.jsonl` (append-only JSONL) +
  `archive-<ts>.jsonl` (rotated on reset). 11 offline unit tests.
- `commands.rs`: `AppState.claude_sessions` restructured to the two-layer lock
  (`Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<ClaudeSession>>>>`) — outer
  std Mutex guards the map for microseconds (never across `.await`), inner
  per-project tokio Mutex holds for a full turn so projects run concurrently
  while same-project turns serialize. `with_session` (get-or-spawn, returns
  the Arc), `build_next_loop_prompt` (first unchecked `- [ ]` under
  `## Next Steps`, raw read since `memory::parse_loops` drops the
  checked/unchecked distinction), and `send_and_record` (append user turn
  before send → send → append assistant turn — crash-safe). Four Tauri
  commands: `agent_start_loop`, `agent_send_message`, `agent_get_conversation`,
  `agent_reset_session`.
- `agents.rs`: `UsageInfo` made `pub` + `Deserialize` + `PartialEq` so
  `ConversationTurn` can carry and round-trip it.

**Frontend.**
- `ProjectCard.tsx`: prominent **Start** CTA (Play icon, primary) above the
  icon-tile row.
- `Dashboard.tsx`: wires `onStart` → `setSelectedProject` +
  `setDetailTab("agent")` + `setPendingAgentStart(path)`.
- `ProjectDetail.tsx`: local `useState<DetailTab>` lifted into the store
  (`detailTab` / `setDetailTab`); new `agent` tab (Bot icon) renders
  `<AgentPanel>`.
- `AgentPanel.tsx` (NEW): loads transcript on mount; auto-fires
  `agent_start_loop` when `pendingAgentStart` matches; renders chat bubbles
  (user/assistant/error styling, usage + duration meta); Start / Send
  (free-form, Enter-to-send) / New conversation controls; spinner while busy.
- `types/index.ts`, `lib/tauri.ts`, `store/appStore.ts`: `"agent"` tab,
  `ConversationTurn` / `AgentResponse` / `UsageInfo` types, four typed IPC
  wrappers, `detailTab` + `pendingAgentStart` store state.

**Verification.** `cargo check` 0 errors; `cargo clippy` 0 new warnings (8
pre-existing untouched); offline `cargo test --lib` 108 passed; live
`cargo test --lib claude_session -- --ignored` 4/4 passed (existing 3 +
resume); `tsc --noEmit` clean. Manual `tauri dev` UI verification left to the
user.

Closes loops.md #4 (per-project turn lock), #5 (`with_session`), #6
(`agent_send_message`).

Files changed: src-tauri/src/{claude_session.rs, conversation.rs (new),
commands.rs, agents.rs, lib.rs}, src/{types/index.ts, lib/tauri.ts,
store/appStore.ts, components/dashboard/{ProjectCard.tsx, Dashboard.tsx},
components/detail/{ProjectDetail.tsx, AgentPanel.tsx (new)}},
.loopdeck/loops.md.

### 2026-07-03 — ClaudeSession resilience hardening (checkpoint 4)
- **Status**: completed
- **Completed**: 2026-07-03

Closed the three foundation gaps flagged in checkpoint 3, plus a real Drop
bug found via clippy along the way.

- **stderr-drain task**: `Stdio::null()` → `Stdio::piped()` + a background
  `tokio::spawn` that loops `read_line` on stderr and logs each non-empty line
  at debug (`[claude stderr] …`), exiting on EOF. Eliminates the latent
  deadlock where a verbose child fills its OS pipe buffer with nowhere to go.
  Handle stored as `stderr_drain: Option<JoinHandle<()>>`; `Drop` aborts it
  defensively after reaping the child.
- **`send_message` timeout**: wrapped the whole turn (stdin write + read-until-
  `result`) in `tokio::time::timeout` with a new `SEND_MESSAGE_TIMEOUT`
  const (180s). A stuck peer now fails loudly as `AppError::Agent("…
  timed out after 180s")` instead of hanging the caller. Post-timeout the
  session is left mid-turn — documented as "drop, don't resend".
- **Drop restructure (bonus bug fix)**: clippy's `let_underscore_future`
  warning surfaced a real defect — the force-kill path used `let _ =
  self.child.kill()` / `wait()`, but tokio's `kill()`/`wait()` return
  *futures*; dropping them unawaited meant the SIGKILL was never sent and the
  child was leaked, not killed. Rewrote `Drop` into two clear phases (graceful
  5s EOF window → forceful `start_kill()` + 2s reap) sharing a new
  `poll_reap(child, window)` helper (non-blocking `try_wait()` paced with
  `thread::sleep`). `start_kill()` is synchronous — the correct tokio API for
  a sync `Drop`.

Verified: `cargo check` + `cargo clippy` clean for new code (remaining
warnings are pre-existing dead-code that clears once `ClaudeSession` is wired
into a Tauri command). All 3 live integration tests pass (`test_session_single_turn`,
`test_session_current_directory`, `test_session_retains_context_across_turns`) —
the stream-json protocol is unaffected and `Drop` still reaps cleanly.

Next up: #4 per-project turn lock, #5 `with_session` helper, #6 `agent_send_message`
Tauri command.

Files changed: src-tauri/src/claude_session.rs.

### 2026-07-02 — ClaudeSession async/tokio migration (checkpoint 3)
- **Status**: completed
- **Completed**: 2026-07-02

Migrated `ClaudeSession` from `std::process` to `tokio::process` to prepare for
concurrent multi-project sessions and streaming. `send_message` is now `async`,
reading stdout via `AsyncBufReadExt::read_line().await` and writing stdin via
`AsyncWriteExt::write_all().await`. `Drop` rewritten for tokio: `start_kill()`
plus a bounded `try_wait()` reap loop (no `.await` in `Drop`).

Two bugs fixed along the way: (1) missing `\n` on NDJSON writes — lost when
`writeln!` became `write_all` during the migration, it silently broke the input
protocol and stalled reads; surfaced by removing a stray `.ok()` that was
swallowing the write error. (2) `Stdio::piped()` stderr with no reader — latent
deadlock risk, switched to `Stdio::null()` for now (proper drain task is a
next step).

Also introduced a `CommandEnv` trait so `apply_agent_config` is generic over
both std and tokio `Command` — keeps the offline env-var inspection tests
working via std's `get_envs()` while production uses tokio. `spawn` now takes
`project_path` and calls `cmd.current_dir(project_path)` so claude runs in the
project, not in loopdeck's cwd.

Live integration tests pass against the provider (single-turn, current-directory,
and cross-turn context retention). Commits `4a10df9` (config foundation) and
`192ac8a` (ClaudeSession) on `feat/claude-session`.

Files changed: src-tauri/src/{claude_session.rs, agents.rs, commands.rs, lib.rs,
config.rs}, src/{types/index.ts, lib/tauri.ts, App.tsx, components/{layout/AppShell.tsx, settings/Settings.tsx}}.

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
