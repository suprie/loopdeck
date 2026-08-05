---
prd: prd-rebrand-tokens
epic: selasar-revamp
milestone: "0.5.0"
status: proposed
description: >
  Replace the app's Tailwind-v4 design tokens with the mockup's paper/ink/
  teak palette and serif display font (light default, full dark pair), and
  rename the app from LoopDeck to Selasar across every user-visible surface
  — window title, wordmark, in-app copy — without touching the on-disk
  `.loopdeck/` directory name or `project.yaml` schema.
---

# PRD — Rebrand & Design Tokens

## Overview

The lowest-risk, first-delivered PRD of the `selasar-revamp` epic: a
palette/typography swap plus a full-repo rename pass. Every later PRD in this
epic (rail, corridor, drawer, night-run surfaces) renders against these
tokens, so this PRD ships first and de-risks the visual language before any
structural IA change begins.

## Problem Statement

- `src/styles.css` currently defines a dark-first Tailwind-v4 token set
  (`--color-background`, `--color-primary`, etc.) with Inter + JetBrains
  Mono — the mockup specifies a warm paper/ink palette (light default,
  `#F5F1E8` paper / `#241E17` ink / `#8A5A3A` teak accent) with a full dark
  pair already defined, plus a serif display font for the wordmark and
  project names.
- "LoopDeck" is very likely hardcoded in more places than the obvious
  sidebar wordmark — window config (`tauri.conf.json`), error strings,
  empty states, Settings copy — and a partial rename reads worse than no
  rename at all.
- The existing light/dark/system theme toggle (`useTheme`, `lib/theme.ts`)
  must keep working against the new token values; nothing about the toggle
  mechanism itself changes.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Replace `styles.css`'s palette values (light + dark) with the mockup's paper/ink/teak/brass/amber/blue/moss/indigo/violet/danger set, keeping existing `--color-*` variable names so no consuming Tailwind class needs to change. |
| P0 | Add a `--font-display` serif token (`ui-serif, Charter, 'Iowan Old Style', 'Palatino Linotype', Georgia, serif`) and apply it to the wordmark, rail mark, and project-name treatments. |
| P0 | Rename the app to Selasar across window title, wordmark, and all user-visible in-app copy. |
| P1 | Full-repo audit checklist of every user-visible "LoopDeck" occurrence, so nothing is missed silently. |
| P2 | Update `docs/` references to the app's name where they describe user-facing behavior (not internal module/identifier names). |

## Non-Goals

- Any change to the `.loopdeck/` directory name, `project.yaml` schema, or
  any other persisted/on-disk format — this is skin and copy only.
- Renaming Rust modules, internal identifiers, crate name, or repository
  name.
- Any structural or navigation change — that's `prd-rail-corridor-shell` and
  `prd-detail-drawer`.

## Design

_Stub — the exact token-to-variable mapping and the full list of "LoopDeck"
occurrence sites are Phase 1/Phase 2 outputs, not decided yet._

## Phases

### Phase 1 — Design tokens

- [x] Replace `src/styles.css`'s light and dark token values with the
      mockup's paper/ink/teak palette, keeping the existing `--color-*`
      variable names unchanged.
- [x] Add a `--font-display` token for the serif display font and apply it
      to the wordmark, rail mark, and project-name text treatments.
- [x] Verify the existing light/dark/system theme toggle renders correctly
      against the new values in all three modes.

### Phase 2 — Rebrand copy audit

- [x] Grep the repo for user-visible "LoopDeck" occurrences (component
      copy, `tauri.conf.json` window title/product name, Settings, empty
      states, error strings) and record the full list.
- [x] Update `src-tauri/tauri.conf.json`'s window title and product name to
      "Selasar".
- [x] Update all in-app user-visible copy found in the audit to "Selasar",
      leaving `.loopdeck/` directory name, `project.yaml` schema, and
      internal Rust/TS identifiers untouched.

### Phase 3 — Verification

- [x] Visual pass in light, dark, and system-auto against every existing
      screen (Dashboard, ProjectDetail tabs, Settings, Activity, Loops,
      Decisions, Epics), confirming no contrast regression on destructive,
      warning, or focus states.
- [x] `npx tsc --noEmit` clean; manual smoke confirming "Selasar" appears in
      the window title bar.

## Open Questions

- Does the rename extend to the macOS app bundle identifier / icon asset,
  or only the in-window title and in-app copy? Check during Phase 2.
- Are there `docs/` or `README.md` references that should stay "LoopDeck"
  because they describe the git repository itself rather than the running
  app? Resolve case-by-case during the Phase 2 audit.

## Phase 2 Audit — "LoopDeck" occurrences (2026-08-05)

Full-repo grep of every `LoopDeck`/`loopdeck` occurrence, tagged by whether it
is user-visible copy, a comment, a test assertion, internal/on-disk state,
docs, or tooling. Loops 2–3 rename the **User-visible** set; the **Comments**
set is renamed for consistency per the Phase-2 interview; the rest is left
intact. This list is the rename source of truth for the Phase-2 update loops.

### User-visible copy — renamed to "Selasar"
- `index.html:6` — `<title>LoopDeck</title>` (drives the window title bar)
- `src-tauri/tauri.conf.json:3` — `"productName": "LoopDeck"` (product/bundle name)
- `src-tauri/tauri.conf.json:16` — window `"title": "LoopDeck"`
- `src-tauri/tauri.conf.json:5` — bundle `"identifier": "com.loopdeck.app"` → `com.selasar.app` (Phase-2 open-question resolution)
- `src/components/layout/AppShell.tsx:138` — sidebar wordmark text `LoopDeck`
- `src/components/shared/RootErrorBoundary.tsx:106` — "LoopDeck ran into a problem"
- `src/components/settings/Settings.tsx:161` — agent-harness hint "…the local CLI LoopDeck uses…"
- `src/components/import/ImportFlow.tsx:107` — "Pick a folder on disk. LoopDeck discovers repositories…"
- `src/components/import/NewProjectDialog.tsx:67` — "Create a fresh folder with LoopDeck project memory…"
- `src/components/detail/KnowledgeGraphPanel.tsx:100` — "…LoopDeck only reads the output…"
- `src/components/dashboard/EmptyState.tsx:18` — "…LoopDeck stores context inside each repository."
- `src-tauri/src/commands/agent.rs:1000,1011` — agent initial prompt "You are working on this LoopDeck project…"
- `src-tauri/src/run_executor.rs:152,157` — unattended-run agent prompt "…this LoopDeck project…"
- `src-tauri/src/agents.rs:844` — chat truncation marker "[LoopDeck: response truncated — …]"
- `src-tauri/src/codex_session.rs:655,745` — Codex turn error "LoopDeck does not implement …"
- `src-tauri/src/codex_session.rs:1254` — Codex `clientInfo.title: "LoopDeck"`
- `src-tauri/src/commands/run_queue.rs:1118,1127,1286,1403,1429,1436` — notification titles "LoopDeck run killed / completed / parked"
- `src-tauri/src/execution.rs:608-609` — schema-version error "newer than this LoopDeck supports…upgrade LoopDeck…"
- `src-tauri/src/permission.rs:145` — deny reason "no matching allow rule and LoopDeck is deny-by-default"
- `src-tauri/src/progress.rs:302` — exported summary header "<!-- Generated by LoopDeck… -->"
- `templates/hooks/loopdeck-stop-hook.py:24` — transcript nudge "LoopDeck: if you made an architectural decision…"
- `templates/hooks/loopdeck-decisions-cap.py:72` — transcript message "LoopDeck: auto-archived N decision(s)…"
- `templates/hooks/orchestrator-start.py:58` — additional-context label "LoopDeck: .loopdeck/current-loop.md → …"
- `templates/hooks/loopdeck-memory-write.sh:68` — auto-written decisions.md context "…active on LoopDeck development."

### Release tooling — updated to match renamed artifact
- `scripts/smoke-test-release.sh` — `LoopDeck.app` / `Contents/MacOS/LoopDeck` / `LoopDeck_*_aarch64.dmg` / `com.loopdeck.app` references updated to `Selasar.*` / `com.selasar.app` to match Loop 2's productName/identifier change; the real config-dir path `com.loopdeck.LoopDeck` (`:114`) is preserved (that on-disk path is unchanged).

### Comments — renamed for consistency
- Frontend: `src/components/shared/RootErrorBoundary.tsx:26`, `src/components/shared/PermissionModeBadge.tsx:4`, `src/components/shared/Markdown.tsx:15`, `src/components/detail/Chat.tsx:218`, `src/components/detail/ProjectDetail.tsx:139`, `src/hooks/useStuckSessions.ts:24`, `src/types/index.ts:137,434,844,865`, `src/styles.css:175,213`
- Backend: `src-tauri/src/retry.rs:3`, `codex_session.rs:4,650,1245,1247,1264,1342`, `binary.rs:8`, `persist.rs:1`, `execution.rs:23`, `graphify.rs:8`, `harness.rs:3,93`, `agents.rs:180,205`, `runplan.rs:78`, `permission.rs:1,4,178,685,1250`, `scanner.rs:226`, `conversation.rs:143`, `claude_session.rs:531,541,1386`, `commands/agent.rs:552`, `commands/project.rs:359,530`, `skills.rs:517,1156`
- Hook-script docstrings/comments: `templates/hooks/loopdeck-stop-hook.py`, `loopdeck-memory-write.sh`, `loopdeck-dirty-flag.py`, `orchestrator-start.py`, `loopdeck-decisions-cap.py` (all `:2`-ish docstrings + inline "LoopDeck-tracked" gate comments)

### Test assertions — updated to match renamed copy
- `src-tauri/src/skills.rs:1249` — `assert!(stop_content.contains("LoopDeck Stop hook"))` → `"Selasar Stop hook"`
- `src-tauri/src/skills.rs:1254` — `assert!(mem_content.contains("LoopDeck memory auto-write"))` → `"Selasar memory auto-write"`

### Left intact — non-copy, on-disk paths, internal identifiers
- `src-tauri/src/config.rs:617` — `ProjectDirs::from("com","loopdeck","LoopDeck")` (global registry at `~/.config/loopdeck/config.yaml`); doc comments at `config.rs:435,501,614` and `secrets.rs:12` reference the same on-disk path
- `src-tauri/src/logging.rs:48` — `APP_NAME = "LoopDeck"` + log-dir comments (`logging.rs:10,190,193,212`) → `~/Library/Logs/LoopDeck/`
- `src-tauri/src/git.rs:495,541,586,730`, `multi_agent.rs:963`, `secret_scan.rs:210`, `run_queue.rs:1844` — `git config user.name "LoopDeck Test"` test fixtures (never user-visible)
- `.loopdeck/` directory name + `project.yaml` schema references throughout — persisted on-disk format
- Lowercase `loopdeck` identifiers: crate/package names (`Cargo.toml`, `package.json`, `package-lock.json`), IPC/store keys, skill names (`loopdeck-orchestrator`, …), api wrapper names — internal identifiers
- `src-tauri/src/codex_session.rs:1253` — `clientInfo.name: "loopdeck"` (protocol identifier; only the display `title` is renamed)

### Docs & tooling — out of Phase-2 scope (P2 goal)
- `README.md`, `AGENTS.md`, `CLAUDE.md`, `SECURITY.md`, `docs/**`, `docs/epics/**` — project docs; left "LoopDeck" where they describe the repo/module rather than the running app
- `templates/skills/loopdeck-*`, `.agents/skills/loopdeck-*` — developer skills; internal tooling, not in-app copy
