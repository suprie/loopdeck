# PRD — Pixel Perfect Clone Redesign

> **Source of truth:** `design/Pixel Perfect Clone/` (a Lovable-exported TanStack Start web mock)
> **Target:** `src/` of the LoopDeck Tauri desktop app
> **Status:** Proposed (2026-07-05)

Redesign every page in the desktop app to match the **Pixel Perfect Clone** — the Lovable mockup that establishes the visual language for LoopDeck going forward. The clone is a *visual and structural* reference; the desktop app keeps its richer Tauri-backed functionality but adopts the clone's layout, design tokens, component primitives, and per-page anatomy.

This PRD inventories what exists today, what the clone specifies, the gap between them, and the work to close it — page by page.

---

## 1. Context & Constraints

| Aspect | Clone (`design/Pixel Perfect Clone`) | App (`src/`) | Decision |
|---|---|---|---|
| Shell | TanStack **Start** (SSR/web), `createFileRoute`, browser history, `<html>` shell in `__root.tsx` | Tauri v2 desktop, **code-based** routes, **memory** history | **Keep current routing.** Adopt only the visual design. No SSR, no file-based routes. |
| Stack | React + TanStack Router + TanStack Query | React + TanStack Router + Zustand + Tauri IPC | Unchanged. |
| Theme | Full light **and** dark palettes, `ThemeProvider` with `light/auto/dark` toggle | Dark-only (`:root` holds dark tokens; `.dark` is a no-op) | Adopt the dual-palette system + theme toggle. |
| Data | `mock-data.ts` (6 projects, sample chat) | Real data via `lib/tauri.ts` IPC wrappers | Unchanged — keep all live functionality. |
| Fonts | `@fontsource/inter` + `@fontsource/jetbrains-mono` npm packages | Google Fonts `<link>` in `index.html` | Switch to bundled `@fontsource/*` for offline-first guarantee. |
| Primitives | `shadcn/ui` set in `components/ui/` (button, dialog, select, tooltip, …) | Bespoke components, no shared primitive layer | **Adopt the primitive layer** and rebuild bespoke components on top. |

### Guiding principles

1. **Pixel alignment is the goal, not "inspired by."** Spacing, type scale, color tokens, and component anatomy should match the clone.
2. **Functionality is preserved.** The clone is a static mock; the app has streaming agent runs, search/filter, conversation history, markdown rendering, manual approvals, etc. None of that is dropped. Where the clone shows a lighter pattern (e.g. plain Activity timeline), the app keeps its richer behavior *but adopts the clone's visual treatment*.
3. **The clone's tokens and primitives become the foundation.** Rebuild on top of them; don't fork a second design system.

---

## 2. What's Missing — Executive Summary

The current app and the clone agree on **what screens exist** (Dashboard, Activity, Agent, Decisions, Loops, Import, Settings, Project Detail) and roughly on the sidebar shell. The gaps are almost entirely in **visual polish and consistency**:

| Theme | Gap |
|---|---|
| **Design tokens** | No light-mode palette; `--shadow-*` undefined; `--accent-foreground`, `--secondary-foreground`, `--popover-foreground`, `--card-foreground` not mapped; `--warning` declared in `@theme inline` but missing from `:root`. |
| **Component primitives** | No shared `shadcn/ui` primitive layer — every page rebuilds buttons, inputs, cards, selects, badges, fields from scratch with drifting class strings. |
| **Theme switching** | No UI affordance; dark is forced. Clone ships a `light / auto / dark` toggle in the sidebar footer. |
| **Typography & radius utilities** | Clone defines `card-accent-top` and `nav-active-bar` custom utilities; app has neither. |
| **ProjectCard** | Misses the **RunState button** (`idle/working/waiting/done` with spin animation), the **`card-accent-top` gradient bar**, the **uncommitted-diff row**, and the **monogram avatar with per-project gradient**. App uses a single-character hash-color tile instead. |
| **ProjectDetail** | Clone uses a **two-column shell** (left tab rail + right content) with a centered `max-w-2xl` overview card built from a reusable `Section` primitive. App uses an ad-hoc layout with inline string section headers and no shared `Section`. |
| **Agent page** | Clone shows a polished **chat surface** (user/assistant bubbles, avatars, streaming cursor, `<details>` thinking drawer, inline tool-call lines, approval card, token meta). App's `/agent` route is a **terminal-style runner** with no chat bubbles. The richer `Chat` component exists but is only used inside ProjectDetail's Agent tab — not on the top-level Agent page. |
| **Activity / Decisions / Loops** | Clone uses a consistent **card list** with `rounded-xl border bg-card shadow-[var(--shadow-sm)]` and `text-[10px] uppercase tracking-wider` micro-labels. App has heavier bespoke rows with its own search/filter bars (functionality to keep) but inconsistent card treatment. |
| **Settings** | Clone uses `Field` + `Select` primitives with `focus:border-primary/50 focus:ring-2 focus:ring-ring/20` focus treatment and a "Configuration saved successfully" inline success toast. App's inputs lack the consistent focus ring. |
| **Import** | Clone shows two **ImportCards** (Scan folder / Clone remote) + a drag-drop dropzone. App only supports folder scanning — clone-remote and drag-drop are new entry points to add. |
| **404 / Error** | Clone has dedicated `NotFoundComponent` and `ErrorComponent` in the root route. App has none. |

---

## 3. Design System Foundation (do this first)

This section is the prerequisite for every page-level change. Land it as one PR before touching individual pages.

### 3.1 Tokens — replace `src/styles.css`

Replace the dark-first `:root` block with the **two-palette** system from the clone (`design/Pixel Perfect Clone/src/styles.css`). Concretely:

- `:root` holds the **light** palette (background `oklch(0.99 0.003 270)`, etc.).
- `.dark` holds the **dark** palette (the values currently in `:root`).
- Add the missing `--shadow-sm / --shadow-md / --shadow-lg` definitions for both schemes.
- Map every `--color-*` alias in `@theme inline` (`--color-surface`, `--color-surface-elevated`, `--color-card-foreground`, `--color-popover-foreground`, `--color-secondary-foreground`, `--color-accent-foreground`, `--color-success`, `--color-warning`).
- Add `--warning` to `:root` (declared in `@theme inline` today but undefined in `:root` → resolves to nothing).
- Add the two `@utility` declarations: `card-accent-top` and `nav-active-bar`.
- Preserve the existing `.hljs-*` syntax-highlight block (still needed by `Markdown.tsx`).

> Reference: `design/Pixel Perfect Clone/src/styles.css` lines 42–164 are the canonical token block.

### 3.2 Fonts — bundle, don't link

- Add `@fontsource/inter` (weights 400/500/600/700) and `@fontsource/jetbrains-mono` (400/500) as dependencies.
- Import them once at the top of `src/main.tsx` (the clone imports in `__root.tsx`; `main.tsx` is the Tauri equivalent).
- Remove the Google Fonts `<link>` tags from `index.html` so the app is fully offline-capable.

### 3.3 ThemeProvider + toggle

- Port `design/Pixel Perfect Clone/src/lib/theme.tsx` to `src/lib/theme.tsx` verbatim (`light / auto / dark`, `localStorage` persistence, system-media listener).
- Wrap the router root with `<ThemeProvider>` (in `App.tsx`).
- Port the `ThemeToggle` (Sun/Monitor/Moon segmented control) from `app-shell.tsx` into the sidebar footer.
- Today `router.tsx` hardcodes `<div className="dark …">` on the shell — remove the hardcoded class so the provider can drive it.

### 3.4 Component primitives — `src/components/ui/`

Copy the `shadcn/ui` primitive set from `design/Pixel Perfect Clone/src/components/ui/` into `src/components/ui/`. At minimum the set the pages depend on:

- `button`, `input`, `textarea`, `label`, `select`, `badge`, `card`, `dialog`, `alert-dialog`, `dropdown-menu`, `tooltip`, `tabs`, `separator`, `skeleton`, `scroll-area`, `sheet`, `popover`, `command`, `sonner` (toast), `switch`, `checkbox`, `avatar`, `progress`.

Add `class-variance-authority`, `@radix-ui/*` packages, and `sonner` to `package.json` (the clone's `package.json` is the reference for versions). Wire the existing `cn()` helper (`src/lib/utils.ts` doesn't exist yet — port `design/Pixel Perfect Clone/src/lib/utils.ts`).

Then add two LoopDeck-specific components the clone defines:

- `src/components/status-badge.tsx` — the shared `StatusBadge` used by `ProjectCard` and `ProjectDetail`.
- `src/components/project-card.tsx` — see §4.2.

### 3.5 AppShell — port the clone's shell

Replace the bespoke sidebar in `src/router.tsx::AppShellLayout` with the clone's `AppShell` (`design/Pixel Perfect Clone/src/components/app-shell.tsx`):

- `nav-active-bar` left accent on the active link.
- `PageHeader` with `border-b px-8 py-5` and `text-lg font-semibold tracking-tight` title (today's `PageHeader` uses `h-14 px-6 text-sm` — different proportions).
- Footer row with `ThemeToggle` + version + `⌘K` kbd hint.

> **Functional note:** today the sidebar's "Import Repo" item triggers a native folder dialog (Tauri `open({ directory: true })`) instead of navigating. That behavior must be preserved — the clone's plain `<Link to="/import">` is **not** a drop-in. Keep the `onClick` intercept.

`PageHeader` is currently exported from `src/components/layout/AppShell.tsx`; consolidate it into the ported `AppShell` to match the clone's single source.

---

## 4. Page-by-Page Work

Each page section below lists **Status today → Clone target → Work items**.

### 4.1 Dashboard (`/`)

**Today** — `Dashboard.tsx` renders a `grid-cols-1 md:grid-cols-2 xl:grid-cols-3` of bespoke `ProjectCard`s, plus an `EmptyState`. Functionality (start agent, rescan, open in Finder/Terminal, remove) is wired.

**Clone target** — `routes/index.tsx`: a `grid-cols-[repeat(auto-fill,minmax(280px,1fr))]` of clone `ProjectCard`s, with a "Preview empty state" toggle in the header actions and a primary "Import Repo" CTA.

**Work items**

- [ ] Switch the grid to `auto-fill, minmax(280px,1fr)` so cards pack responsively.
- [ ] Add the secondary "Preview empty state" toggle next to the primary Import CTA in the header actions (useful for screenshots/demo; gates `EmptyState`).
- [ ] Adopt the clone's `EmptyState` copy and iconography (`FolderOpen` in a dashed `rounded-2xl` tile).

### 4.2 ProjectCard — full rewrite

**Today** — `dashboard/ProjectCard.tsx` is a memoized bespoke card with: hash-color monogram tile, freshness-tinted commit/modified/loop rows, a `Circle` status dot, a full-width "Start" CTA, and a 4-button action footer (Rescan / Finder / Terminal / Remove). Rich, but visually heavier than the clone and missing several clone elements.

**Clone target** — `components/project-card.tsx`:

- `card-accent-top` gradient bar across the top (driven by `--tw-gradient-from/to` per project).
- **Monogram avatar** with per-project `accentFrom → accentTo` linear gradient (not a hash-color tile).
- `ArrowUpRight` that fades in on hover.
- **`RunButton`** that reflects `RunState`: `idle→Start (primary)`, `working→Working (blue, spinner)`, `waiting→Waiting (amber, MessageCircleQuestion)`, `done→Done (emerald, Check)`. Today's card has a plain "Start" button only.
- Single inset `bg-muted/40` info panel containing: last commit (with `GitCommitHorizontal`), folder modified (`Clock`), **uncommitted diff row** (`FileDiff` with `+added / −deleted`), and an optional current-loop row.
- `StatusBadge` (shared component) instead of a `Circle` dot.
- 4-icon action footer (Rescan / Finder / Terminal / Remove) in a `border-t` row — visually identical to today's, adopt the clone's classes.

**Work items**

- [ ] Add `RunState` to the project data model. Today `ProjectEntry` has no run state; the closest signal is "agent session active within 30 min" computed in `AgentRunner.tsx`. Expose run state per project (see §6 Backend).
- [ ] Add `monogram`, `accentFrom`, `accentTo`, `uncommitted { files, added, deleted }` to the project payload (or compute monogram + accents on the client; `uncommitted` needs a backend git-diff-summary command).
- [ ] Rebuild `ProjectCard` on the `Card` + `Button` primitives with the `card-accent-top` utility and the four-state `RunButton`.
- [ ] Preserve all current handlers (`onSelect`, `onStart`, `onRescan`, `onOpenInFinder`, `onOpenInTerminal`, `onRemove`) — the clone's card is a `<Link>`, but in the Tauri app the card drives memory-history navigation via `onSelect`. Keep the existing interaction model; only restyle.

### 4.3 Agent page (`/agent`) — biggest visual delta

**Today** — `AgentRunner.tsx` is a **two-pane terminal runner**: left = session list (per-project transcripts with live/idle dots), right = monospace terminal output with `❯ user` / `❯ assistant` prompt lines, a `Run next loop` toolbar, and a terminal-style composer. A separate `RunnerQuestionPrompt` renders `AskUserQuestion` as a numbered monospace menu.

**Clone target** — `routes/agent.tsx` is a **chat surface**:

- User messages: right-aligned bubble, `bg-primary` rounded with `rounded-br-sm`, `User` avatar.
- Assistant messages: left-aligned, `Bot` avatar, a card-style body (`border bg-card px-4 py-3`) with a **streaming cursor** (`animate-pulse` block).
- `<details>` thinking drawer (`Show thinking (N chars)`).
- Inline tool-call lines (`› Read · path/to/file`).
- Token/cost meta row (`4.2s · 1,204→856 tok · $0.0124`).
- **Approval card** for `permission_request` — primary/secondary "Allow / Deny" buttons with `Shield` icons (today only the in-detail `Chat` surfaces approvals; the runner does not).
- Bottom composer: bordered `textarea` + `Send` button, `Enter to send · Shift+Enter for newline` hint.

The rich `Chat.tsx` component (in `components/detail/`) **already implements** most of this — streaming blocks, approval cards, AskUserQuestion cards, markdown rendering. It is currently only mounted inside ProjectDetail's Agent tab.

**Work items**

- [ ] Decide the model: top-level `/agent` should host the **chat surface**, not the terminal runner. Two viable shapes:
  1. **(Recommended)** Make `/agent` render a project-picker + the existing `Chat` component for the selected project (reusing `Chat.tsx`, which already handles streaming, approvals, AskUserQuestion, markdown). The terminal runner is retired or moved to a power-user toggle.
  2. Keep both: default to chat, offer a "Terminal view" toggle. More work, dual maintenance.
- [ ] Port the clone's `MessageRow`, `Avatar`, approval-card, and composer styles into `Chat.tsx` (or a new `AgentPage` wrapper) so the surface matches the mock.
- [ ] Replace `RunnerQuestionPrompt`'s monospace menu with the card-style `AskUserQuestion` UI already present in `Chat.tsx`.
- [ ] Surface per-turn **thinking**, **tool calls**, and **meta (time/tokens/cost)** in the chat bubble — `Chat.tsx` already has these via `ContentBlock`; ensure they render with the clone's typography.
- [ ] Add a "New chat" header action that calls the existing `agentResetSession` flow.

### 4.4 Project Detail (`/project/$projectPath`)

**Today** — `ProjectDetail.tsx` uses a 180px left nav (`overview / decisions / loops / agent`) and renders the overview as a single ad-hoc card with inline `text-[10px] uppercase tracking-wider` section headers, an `EditDescription` inline form, and a `ConfirmDialog` for removal. Decisions/Loops/Agent are delegated to panel components.

**Clone target** — `routes/projects.$id.tsx`:

- PageHeader title is a **back-button + project name**; subtitle is the mono path; right side carries a `StatusBadge`.
- Left tab rail (w-44) with the project name as a `text-[10px] uppercase` heading and `nav-active-bar` active styling.
- Right content: a centered `max-w-2xl` rounded card built from a reusable **`Section`** primitive (`label + optional actions + children`, separated by `border-b`).
- Below the card: a row of `ActionButton`s (`Rescan`, `Finder`, `Terminal`, `Remove`) with the destructive one tinted.

**Work items**

- [ ] Move the back button into the `PageHeader` title slot (clone pattern) instead of the actions slot.
- [ ] Add `StatusBadge` to the header actions.
- [ ] Extract a shared **`Section`** primitive and rebuild the overview card with it (Path / Description [+ edit/regenerate actions] / Details grid / Repository Activity).
- [ ] Convert the Details row to the clone's `grid-cols-3` (Status / Created / Opened) layout.
- [ ] Rebuild the action row using a shared `ActionButton` (icon + label, `destructive` variant for Remove) — replaces today's ad-hoc muted buttons.
- [ ] Keep `EditDescription`, `ConfirmDialog`, `DecisionsPanel`, `LoopsPanel`, `AgentPanel` behavior intact; restyle their wrappers to match.

### 4.5 Activity (`/activity`)

**Today** — `ActivityFeed.tsx` aggregates real events (agent turns, decisions, loops) across all projects into a date-grouped timeline with expandable rows, color-coded icons, and an event counter. Functionally richer than the clone.

**Clone target** — `routes/activity.tsx`: a simple `max-w-3xl` vertical timeline with a `border-l` left rail, dot+icon markers, and small `bg-card` cards per event (project name + relative time + detail line).

**Work items** (visual alignment only — **keep** search/filter/date-grouping/expandable rows)

- [ ] Re-skin `EventRow` to the clone's card-on-a-left-rail treatment (`border-l border-border pl-6`, dot marker `-left-[27px]`, `rounded-lg border bg-card p-3 shadow-[var(--shadow-sm)]`).
- [ ] Adopt the clone's icon set per event kind (`GitCommitHorizontal`, `Bot`, `RotateCw`, `Activity`).
- [ ] Keep the existing date-bucket headers and expandable detail; they're a superset the clone doesn't have.

### 4.6 Decisions (`/decisions`)

**Today** — `DecisionsView.tsx`: full search + status filter + project filter, grouped by month, expandable cards with Context/Consequences. Far richer than the clone.

**Clone target** — `routes/decisions.tsx`: a flat `max-w-3xl` list of `rounded-xl bg-card` cards (project chip with `GitBranch`, date, title, rationale).

**Work items** (visual alignment only — **keep** filters and grouping)

- [ ] Re-skin the decision card to the clone's anatomy (uppercase project chip + date row, `text-sm font-semibold` title, `text-xs text-muted-foreground` rationale).
- [ ] Use the `GitBranch` project chip from the clone.
- [ ] Keep the status filter, project filter, search, month grouping, and expandable Context/Consequences — the clone is a strict subset.

### 4.7 Loops (`/loops`)

**Today** — `LoopsView.tsx`: per-project expandable cards with current-loop goal, next-steps checklist, history summary, and a stats bar (active loops / pending steps / completed / projects). Richer than the clone.

**Clone target** — `routes/loops.tsx`: a flat `max-w-3xl` list of loop cards, each with a project chip, an `N/M done` counter, a title, and an inline checklist with check/empty boxes (`border-success bg-success/10` for done, line-through text).

**Work items**

- [ ] Re-skin the loop card to the clone's anatomy (project chip + `RotateCw` icon + `N/M done` on the right, title, then the checklist).
- [ ] Render the next-steps checklist with the clone's checkbox styling (the app already parses `- [x]` / `- [ ]` markers — reuse that parsing).
- [ ] Keep the existing expand/collapse, stats bar, and history summary.

### 4.8 Import (`/import`)

**Today** — `ImportFlow.tsx` triggers a native folder scan and lists discovered repos via `RepoCard`. Single entry point (scan local folder).

**Clone target** — `routes/import.tsx`: two `ImportCard`s side-by-side (**Scan local folder** / **Clone remote repo**) plus a full-width dashed **drag-and-drop dropzone**.

**Work items**

- [ ] Add the two-card hero (Scan local folder / Clone remote repo). "Scan local folder" maps to today's flow; "Clone remote repo" is **net-new** — needs a `git clone` backend command and a URL prompt (see §6).
- [ ] Add the dashed **drag-drop dropzone**. Tauri v2 supports `onDragDropEvent` on the webview window; wire it to forward the dropped folder path into `scanFolder`.
- [ ] Keep `RepoCard` rendering for discovered repos below the hero.

### 4.9 Settings (`/settings`)

**Today** — `Settings.tsx`: Auth Token / Base URL / Model / Effort, persisted via `setAgentConfig`, with a save button and transient "Saved" state. Uses a plain `<select>` for Effort and a `<datalist>` for Model presets.

**Clone target** — `routes/settings.tsx`: same fields, but uses a `Field` primitive (label + hint), a `Select` primitive (custom button + `ChevronDown`, not native `<select>`), show/hide token toggle (`Eye / EyeOff`), and a `Check` inline success indicator.

**Work items**

- [ ] Adopt the `Field` primitive (label + hint) across all four fields.
- [ ] Add the **show/hide auth token** toggle (`Eye / EyeOff`) — today the token is always masked.
- [ ] Replace native `<select>` (Effort) and `<datalist>` (Model) with the `Select` primitive from the UI set for visual consistency.
- [ ] Apply the consistent `focus:border-primary/50 focus:ring-2 focus:ring-ring/20` focus treatment to all inputs.
- [ ] Keep the existing `getAgentConfig` / `setAgentConfig` persistence.

### 4.10 404 / Error boundaries (net-new)

**Today** — No not-found or error boundary; an unmatched route silently renders nothing inside the shell.

**Clone target** — `__root.tsx` ships `NotFoundComponent` (big `404`, message, "Go home") and `ErrorComponent` (message + "Try again" + "Go home", reports to `reportLovableError`).

**Work items**

- [ ] Add `notFoundComponent` and `errorComponent` to the root route in `src/router.tsx`.
- [ ] Skip the Lovable error-reporting hook (web-only); optionally log to the Tauri stderr/console.

---

## 5. Shared Component Consolidation

Across pages, replace inline bespoke markup with primitives. Specific consolidations:

| Replace | With |
|---|---|
| Per-page ad-hoc section headers (`text-[10px] uppercase tracking-wider …`) | `Section` primitive (ProjectDetail, Settings) |
| Per-page muted buttons (`inline-flex h-8 … bg-muted text-muted-foreground`) | `Button` variant `ghost`/`outline`/`secondary`/`destructive` |
| Per-page inputs (`h-9 w-full rounded-md border border-input …`) | `Input` primitive |
| `Circle` status dots in ProjectCard | shared `StatusBadge` |
| Bespoke selects / datalists | `Select` primitive |
| `ConfirmDialog` (custom) | `AlertDialog` primitive (keep the existing API or wrap) |

**Cleanup:** delete `src/components/layout/AppShell.tsx` once `PageHeader` is consolidated into the ported `AppShell`.

---

## 6. Backend / Data-Model Touches

Most of the redesign is frontend, but a few clone elements need backend support:

| Need | Source | Action |
|---|---|---|
| `RunState` per project (`idle/working/waiting/done`) | `ProjectEntry` has no such field today | Add to `ProjectEntry`. Derive from whether an agent session is live + last `permission_request` pending + last result. Surfaced via `list_projects`. |
| `uncommitted { files, added, deleted }` per project | `git.rs` tracks dirty + last commit, not diff stats | Extend `git.rs` to compute `git diff --shortstat`-style counts; add to `ProjectEntry`. |
| `monogram`, `accentFrom`, `accentTo` | Pure presentation | Compute on the client (monogram = first letter; accents = hash of name → palette). No backend change. |
| Clone remote repo | New entry point on Import | Add a `clone_repo(url, target_dir)` Tauri command using `gix` or shelling out to `git`. |
| Drag-drop import | New entry point on Import | Frontend-only: Tauri `getCurrentWindow().onDragDropEvent()`. |

---

## 7. Out of Scope

- **Routing architecture.** Stays code-based with memory history. The clone's file-based routes and TanStack Start SSR are not adopted.
- **Mock data.** Not copied in. The app continues to read from `.loopdeck/*` and the global registry via IPC.
- **`reportLovableError` / Lovable telemetry.** Web-only; dropped.
- **Reducing existing functionality.** Search/filter on Decisions, date grouping on Activity, expand/collapse on Loops, the streaming agent + AskUserQuestion + manual approvals — all preserved. The clone is the visual target, not a feature regression.

---

## 8. Sequencing & Milestones

**M0 — Foundation (one PR, blocks everything else)**
- §3.1 tokens, §3.2 fonts, §3.3 ThemeProvider, §3.4 UI primitives + `cn()` + `StatusBadge`, §3.5 AppShell + PageHeader port.

**M1 — High-visibility surfaces**
- §4.2 ProjectCard rewrite (needs §6 RunState + uncommitted).
- §4.1 Dashboard layout.
- §4.4 ProjectDetail Section/ActionButton rebuild.

**M2 — Agent surface**
- §4.3 promote `Chat.tsx` to `/agent`, retire terminal runner (or toggle), port clone chat anatomy.

**M3 — List pages (visual reskin only)**
- §4.5 Activity, §4.6 Decisions, §4.7 Loops.

**M4 — Settings, Import, boundaries**
- §4.9 Settings primitives + token toggle.
- §4.8 Import hero + drag-drop (+ §6 clone_repo).
- §4.10 404/Error boundaries.

**M5 — Consolidation cleanup**
- §5 primitive migration across remaining bespoke components; delete `layout/AppShell.tsx`.

---

## 9. Resolved Decisions (2026-07-05)

1. **Agent page model → retire the terminal runner.** `/agent` becomes a chat surface via the existing `Chat.tsx`. The terminal runner (`AgentRunner.tsx`) is deleted; `RunnerQuestionPrompt` is removed with it. (§4.3)
2. **RunState derivation → strict live state.** `working` = a streaming turn is in flight; `waiting` = a `permission_request` is pending; `done` = last turn finished (transient); `idle` = otherwise. No recency heuristic. (§6)
3. **Clone remote repo → out of scope.** Drop the second ImportCard entirely. The Import page redesign ships only the **Scan local folder** card + the **drag-and-drop dropzone**. (§4.8 / §6)
4. **`ConfirmDialog` → wrap the primitive.** Keep the existing `ConfirmDialog({ title, message, confirmLabel, onConfirm, onCancel, danger })` API; reimplement its internals on top of the `AlertDialog` primitive. Zero call-site churn. (§5)
