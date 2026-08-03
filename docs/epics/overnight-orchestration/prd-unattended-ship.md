---
prd: prd-unattended-ship
epic: overnight-orchestration
milestone: "0.4.0"
status: completed
description: >
  The run's environment and exits: a git worktree per run so the main tree
  stays clean, draft-PR creation with consent given at queue time instead of
  2am, hard token and wall-clock budgets that kill a runaway phase instead of
  letting it burn the night, and a keep-awake assertion so the machine
  doesn't sleep mid-run.
---

# PRD — Unattended Ship

## Overview

`prd-run-queue` sequences the night; this PRD makes the night safe and makes
it end in reviewable artifacts. Four concerns: isolation (worktree-per-run,
ADR-2), shipping (draft PRs under queue-time consent, ADR-1), bounding
(hard budgets, ADR-4), and staying awake (power assertion).

## Current State

Implementation landed on 2026-08-01 in [PR #48](https://github.com/suprie/loopdeck/pull/48)
(`8352538`, followed by the budget enforcement fix in `83a0ec9` and the
macOS import fix in `f976593`). The shipped path now creates a run-scoped
worktree, bootstraps Node dependencies there, executes phases with token and
wall-clock caps, parks non-green results, requires queue-time consent for a
headless draft PR, and holds a macOS `caffeinate` assertion for the run.

The two remaining correctness gaps are now closed:

1. **Secret scan moved before push.** The pattern set that used to live only
   as an inline `rg` pipeline inside the `loopdeck-open-pr` skill markdown —
   run *after* `git push`, so a hit was already on the remote by the time it
   fired — is now a real Rust path: `secret_scan.rs`'s `find_secret` (one
   definition, five named patterns) backs a new `loopdeck secret-scan` CLI
   subcommand (`main.rs`/`lib.rs`, mirroring the existing `loopdeck state`
   convention of routing skill-driven checks through validated Rust). The
   skill's Phase 5a now runs it on the staged diff immediately after
   `git add -A` and *before* the commit — the earliest point a hit can still
   be stopped — with an automated abort test (`run_secret_scan_cli_aborts_on_staged_secret`)
   plus a clean-diff pass test.
2. **PR body metadata completed.** `run_executor::build_phase_prompt` now
   takes a `ResolvedBudgets` (phase token cap, phase/run wall-clock caps,
   resolved from `RunBudgets` defaults in `commands::run_queue::execute_run`)
   and instructs the agent to render a `## Verify Verdict` section (the
   `loopdeck-prd-verifier` report already produced earlier in the same turn,
   copied verbatim) and a `## Run metadata` section (phase id + the resolved
   budget caps) into the unattended draft PR body — both documented in the
   `loopdeck-open-pr` skill's Phase 3 template. Budget-kill coverage now
   exists at the mechanism level: `commands::run_queue::race_with_watchdog`
   (extracted from `execute_run`, mirroring the existing
   `wait_for_bootstrap_child` pattern) is unit-tested against a deliberately
   stuck fixture future, proving the exact timeout-vs-real-work decision the
   executor uses to kill a runaway phase — a live `claude_session` mock
   remains out of reach in this codebase, same known limitation as every
   other executor test. Worktree lifecycle (create / resume / prune-on-success
   / keep-on-failure / keep-and-flag-on-prune-failure) is now covered against
   real temp git repos via `ensure_worktree`/`finalize_worktree` (the latter
   extracted from `execute_run`'s end-of-run cleanup block).

## Problem Statement

1. An overnight session in the main tree collides with the user's own
   uncommitted work and leaves a dirty tree on failure.
2. `agent-full-access` ADR-5 requires interactive confirmation of the PR body
   before `gh pr create` — correct attended, fatal unattended.
3. Nothing bounds spend. `limits.rs` guards untrusted *input*;
   `docs/postmortem-runaway-token-usage.md` shows what an unbounded loop does
   *with a human watching*. Overnight, a stuck phase burns until morning.
4. A closed lid silently freezes the run; the user wakes to nothing.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Worktree per run: created at run start, every phase session runs inside it, main tree untouched; prune on success, keep + flag on failure/kill |
| P0 | Unattended ship: on a green verify verdict, `gh pr create --draft` with no `--web` and no interactive confirm, gated on queue-time consent recorded in the `RunPlan` |
| P0 | Hard budgets: per-phase token cap and wall-clock watchdog, total-run backstop; breach kills the session gracefully, preserves branch + worktree, records the reason |
| P0 | No PR on WARN/BLOCK verify verdicts — record and park instead |
| P1 | Pre-push secret scan of the staged diff; a hit aborts the PR, parks the phase, flags the report |
| P1 | Keep-awake: power assertion held while a run is active, released on completion |
| P2 | Fresh-worktree bootstrap for untracked build deps (e.g. `npm ci`) so verify doesn't fail on a hollow tree |

## Non-Goals

- Auto-ready or auto-merge — drafts stay drafts (epic Non-Goal).
- A new permission tier — runs use the project's configured tier; this PRD
  changes *where* the run happens and *how it exits*, not what it may do.
- Cost *estimation* or budget recommendations — budgets are user-set caps
  with named-constant defaults; predicting spend is out of scope.
- Windows/Linux power assertions — macOS first (the alpha platform);
  the documented "plugged in, lid open" fallback covers the rest.

## Design

Directional; refine during implementation.

- **Worktrees** (`git.rs`): `worktree_add` / `worktree_remove` /
  `worktree_list` wrapping the vetted absolute `git` binary (the existing
  `binary` module path). Branch naming run-scoped, e.g.
  `run/<epic-slug>-<phase-id>-<yyyymmdd>`. The executor passes the worktree
  path as the session's project root; `paths::resolve_within` boundary
  helpers apply to the worktree root for the run's duration.
- **Draft PR**: extend the ship step the orchestrator already performs, with
  an unattended branch: consent flag present in the `RunPlan` → build the PR
  body (PRD link, verify verdict table, `.loopdeck/` memory summary, run
  metadata) → secret-scan the staged diff → push → `gh pr create --draft`.
  Any pre-flight failure (gh auth, base branch, push) parks the phase with
  the error — never retries destructively.
- **Budgets**: token usage read from the streaming usage events the session
  already emits; wall-clock via a watchdog on the executor task. Kill path
  reuses the graceful EOF-then-SIGKILL reap that `claude_session`'s Drop
  already implements. Defaults as named constants in the `limits.rs` stance
  (visible, tunable, one module); per-run overrides come from the plan.
- **Keep-awake**: macOS power assertion (the `caffeinate` mechanism) held by
  the executor while any phase is `running`. If an assertion library proves
  unreliable, fall back to documenting the requirement in the queue UI.

## Phases

### Phase 1 — Worktree lifecycle

- [x] Add `worktree_add` / `worktree_remove` / `worktree_list` to `git.rs` with run-scoped branch naming
- [x] Executor creates the worktree at run start and runs every phase session inside it
- [x] Cleanup policy: prune the worktree after PR creation succeeds; keep it (flagged in the report) on failure or kill
- [x] Bootstrap untracked build deps in fresh worktrees (per-stack, e.g. `npm ci` when `package.json` present)

### Phase 2 — Draft-PR autonomy

- [x] Add an unattended mode to the open-pr flow: `gh pr create --draft`, no `--web`, no interactive confirmation, gated on queue-time consent in the `RunPlan`
- [x] `unattended-ship/pr-body-metadata` PR body: PRD link, verify verdict table, `.loopdeck/` memory summary, run metadata (phase id, budgets used)
- [x] Never open a PR on a WARN or BLOCK verify verdict — record the verdict and park instead
- [x] `unattended-ship/pre-push-secret-scan` Pre-push secret scan of the staged diff for common credential patterns; a hit aborts the PR, parks the phase, and flags the report

### Phase 3 — Hard budgets

- [x] Track per-phase token usage from streaming usage events and enforce a per-phase cap
- [x] Per-phase wall-clock watchdog; breach kills the session via the existing graceful EOF-then-SIGKILL reap path
- [x] Total-run backstop; any breach preserves branch and worktree and records the kill reason
- [x] Budget defaults as named constants beside `limits.rs`, overridable per run from the plan

### Phase 4 — Keep-awake

- [x] Hold a macOS power assertion while a run is active and release it on completion — or document the "plugged in, lid open" requirement in the queue UI if the assertion proves unreliable

### Phase 5 — Tests

- [x] `unattended-ship/budget-kill-test` Budget-kill test with a deliberately stuck fixture phase (proves the epic's top risk mitigation)
- [x] `unattended-ship/worktree-lifecycle-tests` Worktree lifecycle tests: create / run / prune / keep-on-failure; secret-scan abort test

## Open Questions

- **Resolved — budget defaults**: 500,000 tokens per phase, 90 minutes per
  phase, and 8 hours per run; all three are queue-time overrides in `RunPlan`.
- **Resolved — watchdog**: `tokio::time::timeout` enforces the effective
  minimum of the phase deadline and remaining total-run deadline.
- **Resolved — secret-scan ordering**: kept the small hand-rolled v1 pattern
  set, moved into `secret_scan.rs`/`loopdeck secret-scan` (one Rust
  definition instead of markdown-embedded `rg`) and enforced in the
  `loopdeck-open-pr` skill's Phase 5a, immediately after staging and before
  the commit — the earliest point a hit can still be stopped before push.
- **Resolved — power assertion**: spawn `caffeinate -w <pid>` on macOS and
  release it through the run-scoped guard's `Drop` path.
- **Resolved — base branch**: use the repository default in v1; no picker
  override.
