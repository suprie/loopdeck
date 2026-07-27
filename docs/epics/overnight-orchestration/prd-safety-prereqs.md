---
prd: prd-safety-prereqs
epic: overnight-orchestration
milestone: "0.4.0"
status: proposed
description: >
  Harden the destructive floor's mv/cp gap and stand up a CI workflow before
  any unattended run exists — an overnight agent needs the floor watertight,
  and un-eyeballed draft PRs need an automated reviewer's net. Gates the rest
  of the epic (ADR-6).
---

# PRD — Safety Prerequisites

## Overview

Two backlog items (`loops.md` P2 and P3) get promoted into this epic because
overnight autonomy raises their urgency from "should do" to "gates the
milestone": the destructive floor's `mv`/`cp` best-effort gap, and the absence
of any CI. Nothing else in this epic starts until both land (ADR-6).

## Problem Statement

1. **Floor gap.** `check_destructive_floor` (`permission.rs`) argv-analyzes
   the prefix deny-list, but `mv`/`cp` whose *destination* is `/`, `/etc`,
   `/usr`, `/var`, or the `$HOME` root are still best-effort. An attended user
   would catch `mv ~/ /tmp` on the approval card; an unattended run under
   `FullAccess` auto-allows anything that clears the floor — the floor is the
   only line, so the gap must close.
2. **No CI.** Every quality gate today is a local convention (`cargo test`,
   `clippy`, `tsc`). Overnight draft PRs land without a human having watched
   the session; a reviewer opening the PR in the morning needs an independent
   machine-verified signal, not just the agent's own verify report.
3. **Doc drift.** The `claude_session.rs:218-224` doc comment says
   `--permission-mode default` behavior while the code path has drifted
   (`loops.md` P2 flags the `acceptEdits` mismatch). Anyone auditing the
   overnight trust story reads that comment first — it must tell the truth.

## Goals

| Priority | Goal |
|----------|------|
| P0 | `mv`/`cp`/`rsync` with a destination resolving to `/`, `/etc`, `/usr`, `/var`, or the `$HOME` root hard-deny at the floor, with unit tests for absolute, relative, `~`, and `$HOME`-expansion forms |
| P0 | `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `npm ci`, `npx tsc --noEmit`, `npm run build` on every PR |
| P1 | macOS + Ubuntu matrix; cargo/npm caching so cold CI stays under ~10 minutes |
| P1 | `claude_session.rs:218-224` doc comment matches the actual `--permission-mode` argument |
| P2 | CI status badge in `README.md` |

## Non-Goals

- Windows CI matrix (add when a Windows alpha exists; the P3 backlog line
  keeps it).
- E2E/WebdriverIO smoke, vitest, ESLint/Prettier, Dependabot, SBOM — all
  remain P3 backlog; this PRD ships only the gates that overnight PRs need.
- A general argv sandbox or allowlist redesign — the floor stays a deny-list;
  this PRD closes one named gap in it.

## Design

Floor: extend the existing argv analysis in `permission.rs` (the
`check_destructive_floor` path, ~lines 180-493) rather than adding a new
layer. For `mv`/`cp`/`rsync`, resolve the *last* argv path (destination) —
after `~`/`$HOME` expansion, without requiring the path to exist — and deny
when it normalizes to a protected root. Reuse the existing deny-reason
plumbing so audit logging is untouched.

CI: one workflow file, two jobs (rust, frontend), matrix on the rust job.
No new tooling decisions — the commands are exactly the ones CLAUDE.md
already documents as the local gates.

## Phases

### Phase 1 — Destructive floor hardening

- [ ] Extend `check_destructive_floor` argv analysis to deny `mv`/`cp`/`rsync` whose destination resolves to `/`, `/etc`, `/usr`, `/var`, or the `$HOME` root
- [ ] Add floor unit tests for the new denials covering absolute, relative, `~`, and `$HOME`-expansion target forms
- [ ] Reconcile the `claude_session.rs:218-224` doc comment with the actual `--permission-mode` argument

### Phase 2 — CI pipeline

- [ ] Add `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` on a macOS + Ubuntu matrix
- [ ] Add frontend CI jobs: `npm ci`, `npx tsc --noEmit`, `npm run build`
- [ ] Cache cargo and npm artifacts; add a CI status badge to `README.md`

### Phase 3 — Prove the gates

- [ ] Open a throwaway PR with a deliberate clippy warning and confirm CI fails it
- [ ] Spot-check that `mv`/`cp` floor denials appear in the audit log under `FullAccess`

## Open Questions

- Should `/var` denial except `/var/tmp`/`/var/folders` (macOS temp lives
  there)? Current stance: deny the root itself, allow subpaths beneath the
  exceptions — decide during implementation with test cases.
- Does `rsync` belong in the same rule, or is its flag surface (`--delete`)
  worth a separate floor entry?
