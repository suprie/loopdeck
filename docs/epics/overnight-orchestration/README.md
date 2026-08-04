---
title: Overnight Orchestration
slug: overnight-orchestration
milestone: "0.4.0"
status: in_progress
started: 2026-07-27
owner: Suprie
description: >
  Let a user pick phases from a PRD, queue them as an unattended run, and go
  to sleep while LoopDeck executes them sequentially in an isolated worktree
  — verifying each phase against the spec, opening draft PRs on green
  verdicts, and killing runaway phases on hard token and wall-clock budgets —
  so the user wakes to reviewable draft PRs and a morning report instead of a
  stalled approval card.
---

# Epic — Overnight Orchestration

## Motivation

The 0.3.0 `agent-full-access` epic removed per-call friction (the `FullAccess`
tier) and closed the ship step (`prd-verifier` + `open-pr`). But the loop is
still **attended**: the orchestrator is a terminal skill, clarifying questions
and permission cards park the run until a human answers, and ADR-5 of that
epic deliberately requires the user to confirm the PR body before
`gh pr create` runs. "Pick phases and go to sleep" is impossible today — the
run stalls on the first question asked at 2am, and nothing bounds an
unattended runaway (`docs/postmortem-runaway-token-usage.md` documents exactly
that failure shape in an *attended* session; overnight, it compounds for
hours).

This epic moves human interaction to the **edges** of the run:

- **Before sleeping** — a pre-flight interview collects every clarifying
  answer while the user is present, and queue-time consent authorizes draft-PR
  creation for the whole run.
- **While sleeping** — the unattended middle is bounded by the existing
  destructive floor (hardened first), worktree isolation, a stall policy that
  parks instead of waits, and hard token/wall-clock budgets that kill instead
  of burn.
- **After waking** — draft PRs (never auto-readied) plus a morning report
  with per-phase verdicts, parked questions, and the overnight audit slice.

Autonomy stays accountable: nothing ships un-verified, nothing merges without
a human, and every overnight decision is auditable in the morning.

## Scope

In scope:

- **Safety prerequisites, in-epic**: harden the destructive floor's `mv`/`cp`
  gap (targets resolving to `/`, `/etc`, `/usr`, `$HOME` root are currently
  best-effort per `loops.md` P2) and stand up CI — because overnight PRs land
  un-eyeballed and need an automated reviewer's net before the first run.
- A persisted **`RunPlan`** model (`.loopdeck/run-plan.yaml`): ordered phase
  references by stable execution ID, per-phase status, budgets, stall policy,
  and queue-time consent — plus a **sequential queue executor** that spawns
  one orchestrated session per phase via the existing `claude_session` path,
  advancing only on a green verify verdict, resumable across app restarts.
- **Pre-flight interview**: run every queued phase's clarify step up front;
  pin answers into the plan; block run start until answered or skipped.
- **Stall policy**: a mid-run question or permission card parks that phase;
  phases with no dependency on it continue; dependents park transitively.
- **Worktree-per-run** isolation (`git.rs` grows worktree add/remove): the
  main tree stays clean; failed runs keep their worktree for forensics.
- **Draft-PR autonomy**: on a green verdict, `gh pr create --draft` with no
  interactive confirmation — consent was given at queue time (ADR-1).
- **Hard budgets**: per-phase token cap, per-phase wall-clock watchdog, and a
  total-run backstop; breach kills the session, keeps the branch, reports.
- **Wake-up UX**: `tauri-plugin-notification` on run completion / budget kill
  / all-phases-parked, and a morning report view (verdict table, PR links,
  parked-question inbox, overnight audit slice).

Out of scope (deferred or parked):

- **Parallel phase execution** — sequential first; worktrees make parallel a
  later upgrade, not a rewrite (ADR-3).
- **Auto-merging or auto-readying PRs** — drafts stay drafts until a human
  promotes them.
- **OS sandboxing and the `AutonomousProject` path-containment tier** — still
  parked (`loops.md` Parking Lot); worktree + floor + draft-only ship is the
  v1 boundary.
- **Cloud/remote execution** — runs happen on the user's machine.
- **AI-generated PRDs / AI phase decomposition** — still 0.4.0+ per the
  `support-project-management` epic's own deferral; this epic consumes
  authored phases, it does not author them.

## Non-Goals

- **No auto-merge, no auto-ready.** The morning human reviews and promotes
  draft PRs. LoopDeck never runs `gh pr merge` or `gh pr ready`.
- **No new permission tier.** Unattended runs use the project's existing
  configured tier; overnight autonomy comes from queue-time consent plus
  draft-only shipping, not from loosening permissions.
- **No replacement of the terminal orchestrator skill.** The in-app executor
  drives the same phase → verify → ship flow; the skill remains for attended
  terminal use. Divergence between the two is managed, not eliminated, here.
- **No parallelism.** One phase at a time; the RunPlan's dependency edges
  exist for stall-parking correctness, not for scheduling.

## PRD Index

| PRD | Covers |
|-----|--------|
| [prd-safety-prereqs.md](./prd-safety-prereqs.md) | Destructive-floor `mv`/`cp` hardening + floor tests, `claude_session.rs` doc/flag reconciliation, CI workflow (fmt/clippy/test/tsc/build) |
| [prd-run-queue.md](./prd-run-queue.md) | `RunPlan` data model + persistence, sequential queue executor, pre-flight interview, stall-policy runtime, phase-picker UI |
| [prd-unattended-ship.md](./prd-unattended-ship.md) | Worktree-per-run lifecycle, draft-PR autonomy with queue-time consent, hard token/wall-clock budgets, keep-awake |
| [prd-wake-up.md](./prd-wake-up.md) | `tauri-plugin-notification`, morning report view (verdicts, PR links, parked-question inbox, overnight audit slice) |
| [prd-assign-loop-id.md](./prd-assign-loop-id.md) | One-click stable-ID assignment for a loop that lacks one, so it can enter the picker checkbox's existing `execution_id` gate |

**Delivery order is strict — index order, each PRD depends on artifacts of
the previous.** `prd-safety-prereqs` gates everything (ADR-6);
`prd-unattended-ship`'s draft-PR and budget hooks assume `prd-run-queue`'s
executor and `RunPlan` exist; `prd-wake-up`'s report reads the `RunPlan`,
park payloads, and budget usage from both. Do not start a PRD before the
previous one completes. **`prd-assign-loop-id` is the exception** — it only
touches the picker's pre-queue gate (`EpicsPanel.tsx`, `epic.rs`), not the
`RunPlan`/executor artifacts the strict chain above governs, so it can be
picked up independently of where the other four stand.

## Architecture Decisions

Decided in the planning conversation of 2026-07-27; recorded here for review.

### ADR-1: Consent at queue time; draft PRs only

**Context.** `agent-full-access` ADR-5 requires the user to confirm the PR
body before `gh pr create` runs — correct for attended use, fatal for an
overnight run (the confirmation card parks at 2am).

**Decision.** For queued runs, the confirmation moves to **queue time**: the
user authorizes PR creation for the whole run when queuing it ("this run will
open draft PRs"). The unattended ship step runs `gh pr create --draft` — no
`--web`, no interactive confirm. The attended `open-pr` flow is unchanged.

**Consequences.** The "outward-facing actions require confirmation" rule is
preserved by *when* consent happens and *what* ships: consent is explicit and
per-run, and a draft PR is an invitation to review, not a publication — it
cannot merge, and morning review promotes it. Amends (does not rewrite)
`agent-full-access` ADR-5; that epic's README gets a cross-reference.

### ADR-2: Worktree-per-run

**Context.** Overnight sessions running in the main tree would collide with
the user's own uncommitted work and leave a dirty tree on failure.

**Decision.** The executor creates one git worktree per run (run-scoped
branch) and runs every phase session inside it. Prune on success after PR
creation; keep on failure/kill, flagged in the morning report.

**Consequences.** The main tree is untouchable by the run. Fresh worktrees
lack untracked build artifacts (`node_modules` etc.) — the executor must
bootstrap per stack or the verify phase fails early inside the worktree,
which is the safe direction. Parallel runs become a scheduling change later,
not an isolation redesign.

### ADR-3: Sequential executor first; parallel deferred

**Context.** Parallel phase execution multiplies orchestration complexity
(shared-file conflicts, merged budgets, interleaved reports).

**Decision.** One phase at a time, in queue order. Dependency edges in the
RunPlan serve stall-parking (ADR-5), not scheduling.

**Consequences.** A night is bounded by the slowest chain, which is
acceptable for v1. Worktree isolation (ADR-2) already removes the hardest
parallel blocker, so parallelism later is an executor change only.

### ADR-4: Budgets are hard kills, not warnings

**Context.** `docs/postmortem-runaway-token-usage.md` — an unbounded agent
loop burned tokens with a human watching. Overnight, nobody watches.
`limits.rs` bounds untrusted *input*; nothing today bounds *spend*.

**Decision.** Per-phase token cap (tracked from streaming usage events),
per-phase wall-clock watchdog, and a total-run backstop. Breach kills the
session via the existing graceful EOF-then-SIGKILL reap path, preserves the
branch and worktree, and records the reason.

**Consequences.** A stuck phase costs at most its cap, never the night. A
kill is loud (notification + report row), never silent. Defaults live as
named constants beside `limits.rs`'s stance, overridable per run in the plan.

### ADR-5: Interaction at the edges; mid-run stalls park, never wait

**Context.** Clarifying questions, permission cards, and plan approvals are
designed to wait for a human. Overnight, waiting means a dead run.

**Decision.** All expected interaction is front-loaded into the pre-flight
interview. A mid-run interactive event anyway → the phase **parks** with its
question payload. What happens next is a queue-time `StallPolicy` choice:
`continue_independent` (default) skips ahead to queued phases with no
dependency on the parked one, parking dependents transitively; `halt`
preserves strict sequence — a park halts every remaining phase. Parked
questions surface in the morning report, where answering requeues the phase.

**Consequences.** One ambiguous phase cannot kill the night, and stall-parking
never fakes an answer — no auto-answered questions, no silently-skipped
permission checks. The cost is that dependency edges must exist in the
RunPlan (authored order is the default edge).

### ADR-6: Safety prerequisites ship inside the epic

**Context.** The floor's `mv`/`cp` gap and the missing CI workflow live in
the `loops.md` P2/P3 backlog. Overnight autonomy raises their urgency:
un-eyeballed PRs need an automated reviewer's net, and an unattended agent
needs the floor watertight.

**Decision.** Both ship as this epic's first PRD (`prd-safety-prereqs`),
gating the rest — no unattended run before the floor hardening and CI land.

**Consequences.** The epic is self-contained: its safety story does not
depend on backlog items of ambiguous priority. The corresponding P2/P3
backlog lines are satisfied by this PRD when it completes.

## Success Criteria

- From the Epics view, a user can select two or more phases of a PRD, answer
  the pre-flight interview, queue the run, and walk away — and wake to one
  draft PR per completed phase (or a documented park/kill row), having
  answered zero mid-run prompts.
- During an unattended run, a `mv`/`cp` whose destination resolves to `/`,
  `/etc`, `/usr`, or the `$HOME` root hard-denies with the floor reason
  visible in the audit log; all existing floor tests still pass.
- A phase exceeding its token or wall-clock budget is killed within one
  watchdog interval; its branch and worktree are preserved; the morning
  report shows the kill reason and the usage that triggered it.
- A mid-run clarifying question parks only its phase; a queued phase with no
  dependency on it still runs to completion; the question appears in the
  morning report and answering it requeues the parked phase.
- Draft PRs carry the PRD link, the verify verdict table, and a `.loopdeck/`
  memory summary; no PR is opened on a WARN or BLOCK verdict; no PR is ever
  auto-readied or auto-merged.
- An OS notification fires on run completion, on a budget kill, and when all
  remaining phases are parked.
- The CI workflow runs `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`, `npx tsc --noEmit`, and `npm run build` on every PR —
  including agent-drafted ones.
- Existing `config.yaml` and `execution.yaml` files with no run-plan data
  deserialize unchanged; `cargo test`, `cargo clippy -D warnings`, and
  `npx tsc --noEmit` pass on LoopDeck itself.

## Risks

| Risk | Mitigation |
|------|-----------|
| **Runaway spend overnight** — a stuck phase burns tokens for hours with nobody watching (the `postmortem-runaway-token-usage.md` failure shape, compounded) | Hard per-phase token + wall-clock kill (ADR-4), proven against a deliberately-stuck fixture phase *before* the first real overnight run; total-run cap as backstop; kill is loud (notification + report row) |
| Stall policy produces wrong dependent work — a parked phase's unanswered question silently poisons phases built on it | Dependency edges in the RunPlan; dependents park transitively (ADR-5); the verify phase still gates every PR individually |
| Verifier false-positive PASS at volume — morning-you rubber-stamps drafts the verifier green-lit wrongly | Drafts never auto-ready; CI (in-epic) is an independent second gate; the report shows the per-criterion evidence table, not just the verdict |
| Machine sleeps mid-run — laptop lid closes, run silently freezes | Power assertion held while a run is active (macOS `caffeinate`-equivalent), released on completion; fallback: documented "plugged in, lid open" requirement (resolved in `prd-unattended-ship`) |
| Fresh worktree lacks build artifacts — `node_modules` etc. are gitignored, so builds/verify fail in the worktree | Per-stack bootstrap step at worktree creation (e.g. `npm ci`); failures surface inside the worktree *before* any PR, which is the safe direction |
| Draft PR leaks secrets from an overnight diff | Pre-push secret scan of the staged diff (common credential patterns); a hit aborts the PR, parks the phase, and flags the report |
