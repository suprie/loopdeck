---
prd: prd-safety-prereqs
epic: overnight-orchestration
milestone: "0.4.0"
status: completed

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

- [x] Extend `check_destructive_floor` argv analysis to deny `mv`/`cp`/`rsync` whose destination resolves to `/`, `/etc`, `/usr`, `/var`, or the `$HOME` root (2026-07-27) — new `destructive_move_target`/`expand_destination`/`lexically_normalize`/`home_dir` helpers in `permission.rs`; a new `mv`/`gmv`/`cp`/`gcp`/`rsync` arm in `analyze_stage` resolves the last argv token as the destination, expands `~`/`$HOME`/`${HOME}`, lexically collapses `.`/`..` (no filesystem access — the destination need not exist), and hard-denies an exact match against `/`, `/etc`, `/usr`, `/var`, or the resolved home directory. Subpaths (`/var/tmp/x`, `~/Downloads`, `/etc/myapp.conf`) are untouched — see the Open Questions resolution below.
- [x] Add floor unit tests for the new denials covering absolute, relative, `~`, and `$HOME`-expansion target forms — `mv_cp_rsync_to_protected_root_absolute_form_is_caught`, `mv_cp_to_protected_root_relative_dotdot_form_is_caught` (`..`-collapsing, e.g. `/usr/local/../..` → `/`), `mv_cp_to_home_root_tilde_and_home_expansion_forms_are_caught` (`~`, `~/../../home/x`, `$HOME`, `${HOME}`, `${HOME}/`), plus `mv_cp_rsync_to_ordinary_destinations_are_allowed` (negative cases, incl. `/var/tmp`/`/var/folders` subpaths and an unresolvable relative destination). 6 new tests, all green.
- [x] Reconcile the `claude_session.rs:218-224` doc comment with the actual `--permission-mode` argument — already accurate; no change needed. The line range in this PRD is stale (the file has grown since 2026-07-21). The real doc comment (`claude_session.rs:467-474`, next to `cmd.args(["--permission-mode", "default"])`) was corrected back on 2026-07-23 as part of the SECURITY.md work (see `decisions.md` "Add LICENSE and SECURITY.md"), which fixed the exact `default`-vs-`acceptEdits` drift this item describes. `loops.md` P2's mirroring line was stale for the same reason.

### Phase 2 — CI pipeline

- [x] Add `.github/workflows/ci.yml` running `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` on a macOS + Ubuntu matrix (2026-07-27) — `ci.yml` already existed (Release Gate A "Basic CI", 2026-07-23) running all three on macOS only; this PRD's P1 goal was the missing Ubuntu leg. Converted to a `strategy.matrix.os: [macos-latest, ubuntu-latest]`, with an Ubuntu-only `apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf` step ahead of the Rust toolchain (mirrors `build.yml`'s existing Linux dependency step) so `cargo clippy`/`cargo test` can actually compile the Tauri/gtk bindings on Linux. `loops.md` P3's matching line was stale in the same "file exists, box unchecked" way as the two 0.3.0 PRDs reconciled earlier this session.
- [x] Add frontend CI jobs: `npm ci`, `npx tsc --noEmit`, `npm run build` — `npm ci` + `npm run build` present (unchanged); no separate `tsc --noEmit` step because `npm run build` is `tsc && vite build` (`package.json`), so the type-check already runs and fails the job on a type error — a dedicated step would be redundant.
- [x] Cache cargo and npm artifacts; add a CI status badge to `README.md` — `Swatinem/rust-cache@v2` (now keyed per-OS via `key: ${{ matrix.os }}` so the two legs don't collide) and `actions/setup-node@v5`'s built-in `cache: npm` were already present; added the CI badge to `README.md` (top of file, links to the workflow run history).

### Phase 3 — Prove the gates

- [x] Open a throwaway PR with a deliberate clippy warning and confirm CI fails it (2026-07-28) — pushed branch `throwaway/ci-gate-smoke-test` (a `throwaway_ci_smoke_check` fn in `lib.rs` tripping `clippy::bool_comparison` + `clippy::needless_return`, confirmed locally first with `cargo clippy --lib -- -D warnings`), opened PR #27 against `suprie/loopdeck`, watched `gh pr checks 27` — both `fmt · clippy · test · build (macos-latest)` and `(ubuntu-latest)` reported `fail`. Closed the PR with a comment recording the result and deleted the branch (`gh pr close 27 --delete-branch`); nothing merged.
- [x] Spot-check that `mv`/`cp` floor denials appear in the audit log under `FullAccess`/Autonomous — live (2026-07-28) — rather than spin up a full GUI Autonomous session, added a temporary test in `permission.rs`'s test module that constructs `PermissionPolicy::with_mode(PermissionMode::Autonomous)`, calls `decide("Bash", {"command": "mv report.txt /var"})`, and emits the **exact** `tracing::info!` call shape from `claude_session.rs:682-688` (same field list) rather than a paraphrase. Ran it with `cargo test -- --nocapture` and observed the real log line: `behavior="deny" reason="mv destination resolves to protected root \`/var\` blocked by policy floor" autonomous=false`. Confirms the floor deny reaches the identical logging call production code uses, under the Autonomous policy. Test was throwaway — reverted via `git checkout -- src-tauri/src/permission.rs` immediately after, no permanent code change.

## Open Questions

- ~~Should `/var` denial except `/var/tmp`/`/var/folders`~~ **Resolved
  (2026-07-27):** the rule matches the destination **exactly** against the
  protected-root set, never as a prefix — so `/var/tmp/x` and
  `/var/folders/y` (subpaths) were never in scope to begin with, and no
  exception logic was needed. This also means a subpath write into `/etc`
  or `/usr` (e.g. `cp app.conf /etc/myapp/app.conf`) is allowed; only the
  literal root itself as a destination is denied. Documented in
  `destructive_move_target`'s doc comment.
- ~~Does `rsync` belong in the same rule~~ **Resolved (2026-07-27):** yes,
  same rule, same last-argv-is-destination heuristic as `mv`/`cp` — `rsync`'s
  flag surface (`-avz`, `--delete`, etc.) doesn't change which argv token is
  the destination, so no separate floor entry was needed.
