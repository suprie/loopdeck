---
prd: prd-run-queue
epic: overnight-orchestration
milestone: "0.4.0"
status: accepted
description: >
  A persisted RunPlan model and a sequential queue executor: the user selects
  phases in the Epics view, answers a pre-flight interview while present, and
  the executor runs one orchestrated session per phase — advancing on green
  verify verdicts, parking on mid-run stalls instead of waiting, and
  surviving app restarts.
---

# PRD — Run Queue

## Overview

The core of the epic: turn "phases of a PRD" into "an unattended run". A
`RunPlan` (persisted to `.loopdeck/run-plan.yaml`) captures *what* to run,
*with which answers*, *under which consent and budgets*; a sequential
executor inside the app turns the plan into one orchestrated
`claude_session` per phase; a stall policy keeps one ambiguous phase from
killing the night.

## Problem Statement

Today the orchestrator is a terminal skill and in-app agent runs are
interactive chat. There is no way to say "run phases 3–5 of this PRD" and
walk away: nothing sequences phases, nothing collects clarifying answers up
front, and the first mid-run question or permission card parks everything
until a human answers. The structured execution state shipped in 0.2.1
(`execution.yaml`, stable phase IDs, derived progress) gives us addressable
phases — nothing yet consumes them as a queue.

## Goals

| Priority | Goal |
|----------|------|
| P0 | `RunPlan` model persisted atomically to `.loopdeck/run-plan.yaml`, referencing phases by stable execution IDs, carrying queue-time consent, budgets, and stall policy |
| P0 | Sequential executor: one orchestrated session per phase via the existing `claude_session` spawn path; advance only on a green verify verdict; transitions recorded into `execution.yaml` |
| P0 | Pre-flight interview: every queued phase's clarify step runs before the run starts; answers pinned into the plan; run start blocked until answered or skipped |
| P0 | Stall policy, chosen at queue time: mid-run interactive events park the phase (payload recorded); `continue_independent` (default) runs non-dependent phases and parks dependents transitively; `halt` stops every remaining phase (strict sequence) |
| P1 | Resumable across app restarts: `running` phases downgrade to `interrupted` on reload and requeue |
| P1 | Phase picker + live run-queue view in the Epics UI |
| P2 | `retry.rs` backoff reused for transient gateway failures inside a phase |

## Non-Goals

- Parallel execution (ADR-3) — dependency edges exist for parking, not
  scheduling.
- Worktree creation, PR opening, budget enforcement — `prd-unattended-ship`
  owns the run's *environment and exits*; this PRD owns its *sequencing and
  state*.
- Auto-answering questions or synthesizing defaults for mid-run stalls — a
  park is a park (ADR-5); no faked answers, ever.
- Editing epics/PRDs — the queue consumes authored phases; authoring stays
  with `epic-author` and the human.

## Design

Directional; refine during implementation.

- **Model** (`src-tauri/src/runplan.rs`): `RunPlan { id, project, created,
  consent: RunConsent, budgets: RunBudgets, phases: Vec<RunPhase> }`;
  `RunPhase { execution_id, status, interview: Vec<PinnedAnswer>, depends_on,
  park_payload }`. Status enum: `queued | running | parked | completed |
  failed | interrupted | killed`. `StallPolicy` is a two-variant enum chosen
  at queue time: `continue_independent` (default) or `halt` (strict
  sequence — a park halts all remaining phases). Serde defaults throughout so
  absent files and old files deserialize clean; atomic write via the existing
  `persist.rs` path. `depends_on` defaults to the authored order (each phase
  depends on its predecessor) unless the user edits edges in the picker.
- **Executor**: a tokio task owned by app state, mirroring the existing
  streaming-session management in `commands/agent.rs`. It drives the same
  phase → verify flow the orchestrator skill documents, injecting pinned
  interview answers into the session prompt. Green verdict → mark completed,
  advance. Stall event → park, then apply the plan's `StallPolicy`: under
  `continue_independent`, pick the next phase whose `depends_on` chain
  contains no parked/failed phase; under `halt`, park everything remaining.
  **Ordering invariant:** the executor never starts phase N+1 before phase N
  completes, unless N is parked and the policy permits skipping.
- **Interview**: reuse the existing question-card IPC surface — the clarify
  pass is a bounded session per phase whose `AskUserQuestion` payloads render
  as the same cards the chat already shows, answered while the user is
  present.
- **IPC**: `queue_run`, `cancel_run`, `get_run_status` commands + TS wrappers
  in `lib/tauri.ts` and types in `types/index.ts`, matching the existing
  typed-wrapper convention (never raw `invoke()`).

## Phases

### Phase 1 — Run plan data model

- [x] Define `RunPlan`/`RunPhase`/`StallPolicy`/`RunBudgets` structs in a new `src-tauri/src/runplan.rs` with serde defaults and atomic persistence to `.loopdeck/run-plan.yaml` (2026-07-28) — `runplan.rs:37,85,99,121`; `save_to_path` writes via `persist::atomic_write`
- [x] Reference queued phases by stable execution IDs from `execution.rs` (no free-text phase names) (2026-07-28) — `RunPhase.execution_id: String` (`runplan.rs:100`), doc comment cross-references `epic::PrdLoop::id`
- [x] Record queue-time consent (draft-PR authorization, budget values, stall policy) as explicit plan fields (2026-07-28) — `RunPlan.consent`/`.budgets`/`.stall_policy` (`runplan.rs:126,128,130`)
- [x] Unit tests: serde round-trip, missing-field defaults, malformed-file error surface (2026-07-28) — `runplan.rs:205,213,234` (+ save/load-through-disk round trip), all passing

### Phase 2 — Queue executor

- [x] Add a sequential executor task that spawns one orchestrated session per queued phase via the existing `claude_session` spawn path (2026-07-28) — `commands/run_queue.rs::queue_run`'s own async loop calls `commands::agent::start_fresh_and_record` (made `pub(crate)`) once per eligible phase; no separate background-task registry (see decision of same date)
- [x] Advance the queue only on a green verify verdict; record per-phase transitions into `execution.yaml` (2026-07-28) — `run_executor::parse_verdict` greps the turn's final text for the last `**Verdict:** PASS|WARN|BLOCK`; only `PASS` advances via `commands/execution.rs`'s new `promote_by_id`/`complete_with_commit` (`AppState`-free helpers shared with the `promote_loop_by_id`/`complete_current_loop` commands)
- [x] Reuse `retry.rs` backoff for transient gateway failures inside a phase (2026-07-28) — free: `start_fresh_and_record` already wraps `send_with_retry` (`retry::is_overloaded`/`next_backoff`), no new retry code needed
- [x] Make the queue resumable: on app restart, reload the plan and mark previously `running` phases `interrupted` for requeue (2026-07-28) — `run_executor::reconcile_after_restart`, called per registered project in `lib.rs`'s startup, mirroring the existing `conversation::reconcile_interrupted` loop
- [x] IPC commands `queue_run` / `cancel_run` / `get_run_status` plus TS wrappers and types (2026-07-28) — `commands/run_queue.rs` + `lib.rs` registration; `queueRun`/`cancelRun`/`getRunStatus` in `lib/tauri.ts`; `RunPlan`/`RunPhase`/`RunPhaseStatus`/`StallPolicy`/`PinnedAnswer`/`RunConsent`/`RunBudgets` in `types/index.ts`. **Known limitation** (documented in `commands/run_queue.rs`'s module doc): cancellation and stall/park detection both ride the non-streaming pipeline, which doesn't honor the interrupt slot — `cancel_run` takes effect between phases, not mid-turn; Phase 4 owns real park detection + the `StallPolicy` skip-ahead behavior.

### Phase 3 — Pre-flight interview

- [ ] Add an interview pass that runs the orchestrator's clarify step for every queued phase before the run starts, while the user is present
- [ ] Pin interview answers into the `RunPlan` and inject them into each phase's session prompt
- [ ] Block run start until every queued phase's interview is answered or explicitly skipped

### Phase 4 — Stall policy runtime

- [ ] Detect mid-run stalls (question / permission / plan-approval cards) in the streaming event loop and park the phase instead of waiting
- [ ] Under `continue_independent`, continue with the next queued phase that has no dependency on a parked one; park dependents transitively
- [ ] Implement the `halt` stall policy variant: a park halts all remaining phases, preserving strict sequence
- [ ] Record every park with its pending question payload for the morning report

### Phase 5 — Phase picker UI

- [ ] Add phase multi-select and a "Queue overnight run" action to `EpicsPanel.tsx`
- [ ] Add a run-queue view showing live per-phase status (queued/running/parked/completed/failed/killed) via the existing streaming state
- [ ] Interview UI: present pre-flight questions as the existing question cards, gating the "Start run" action

### Phase 6 — Tests

- [ ] Executor state-machine tests: advance-on-green, park-on-stall, transitive dependent parking, halt-policy full stop, resume-after-restart
- [ ] `cargo test` and `npx tsc --noEmit` green; manual smoke of pick → interview → queue → cancel

## Open Questions

- Interview mechanics: is the clarify pass one bounded session per phase, or
  one session covering all queued phases? (Per-phase is simpler and matches
  the orchestrator's flow; all-at-once is cheaper. Decide by trying
  per-phase first.)
- Dependency edges: is authored order (linear chain) enough for v1, or does
  the picker need an explicit edge editor? Default: linear chain, no editor.
- Where does the executor live relative to the existing per-session state in
  `commands/agent.rs` — same registry, or a sibling `RunManager`? Decide when
  touching the code.
- Overlap with the Parking Lot item "Move agent control into LoopDeck app":
  does the executor inject the orchestrator flow as an app-owned prompt, or
  still rely on the on-disk skill? v1: on-disk skill (no new injection
  machinery); revisit with that parking-lot item.
