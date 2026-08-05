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

- [ ] Replace `src/styles.css`'s light and dark token values with the
      mockup's paper/ink/teak palette, keeping the existing `--color-*`
      variable names unchanged.
- [ ] Add a `--font-display` token for the serif display font and apply it
      to the wordmark, rail mark, and project-name text treatments.
- [ ] Verify the existing light/dark/system theme toggle renders correctly
      against the new values in all three modes.

### Phase 2 — Rebrand copy audit

- [ ] Grep the repo for user-visible "LoopDeck" occurrences (component
      copy, `tauri.conf.json` window title/product name, Settings, empty
      states, error strings) and record the full list.
- [ ] Update `src-tauri/tauri.conf.json`'s window title and product name to
      "Selasar".
- [ ] Update all in-app user-visible copy found in the audit to "Selasar",
      leaving `.loopdeck/` directory name, `project.yaml` schema, and
      internal Rust/TS identifiers untouched.

### Phase 3 — Verification

- [ ] Visual pass in light, dark, and system-auto against every existing
      screen (Dashboard, ProjectDetail tabs, Settings, Activity, Loops,
      Decisions, Epics), confirming no contrast regression on destructive,
      warning, or focus states.
- [ ] `npx tsc --noEmit` clean; manual smoke confirming "Selasar" appears in
      the window title bar.

## Open Questions

- Does the rename extend to the macOS app bundle identifier / icon asset,
  or only the in-window title and in-app copy? Check during Phase 2.
- Are there `docs/` or `README.md` references that should stay "LoopDeck"
  because they describe the git repository itself rather than the running
  app? Resolve case-by-case during the Phase 2 audit.
