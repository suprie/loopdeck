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
- **IPC**: `create_run_plan`, `queue_run`, `cancel_run`, `get_run_status`
  commands + TS wrappers in `lib/tauri.ts` and types in `types/index.ts`,
  matching the existing typed-wrapper convention (never raw `invoke()`).

## Phases

### Phase 1 — Run plan data model

- [x] Define `RunPlan`/`RunPhase`/`StallPolicy`/`RunBudgets` structs in a new `src-tauri/src/runplan.rs` with serde defaults and atomic persistence to `.loopdeck/run-plan.yaml` (2026-07-28) — `runplan.rs:37,85,99,121`; `save_to_path` writes via `persist::atomic_write`
- [x] Reference queued phases by stable execution IDs from `execution.rs` (no free-text phase names) (2026-07-28) — `RunPhase.execution_id: String` (`runplan.rs:100`), doc comment cross-references `epic::PrdLoop::id`
- [x] Record queue-time consent (draft-PR authorization, budget values, stall policy) as explicit plan fields (2026-07-28) — `RunPlan.consent`/`.budgets`/`.stall_policy` (`runplan.rs:126,128,130`)
- [x] Unit tests: serde round-trip, missing-field defaults, malformed-file error surface (2026-07-28) — `runplan.rs:205,213,234` (+ save/load-through-disk round trip), all passing

### Phase 2 — Queue executor

- [x] Add a sequential executor task that spawns one orchestrated session per queued phase via the existing `claude_session` spawn path (2026-07-28) — `commands/run_queue.rs::execute_run` calls the (now `pub(crate)`) `commands::agent::start_fresh_and_record` per phase, in the plan's authored `Vec<RunPhase>` order; `queue_run` spawns it via `tokio::spawn` off a cloned `AppHandle` so the IPC call returns immediately
- [x] Advance the queue only on a green verify verdict; record per-phase transitions into `execution.yaml` (2026-07-28) — `run_executor::extract_verdict` greps the turn's final `AgentResponse.result` for `loopdeck-prd-verifier`'s `**Verdict:** PASS|WARN|BLOCK` line (last occurrence, so the skill's own explanatory text can't be mistaken for the roll-up); only `PASS` marks the phase `Completed` and calls `ExecutionState::complete_current`. `WARN`/`BLOCK`/no-verdict/a turn error all mark the phase `Failed` (with the reason in `park_payload`), call `ExecutionState::abandon_current`, and stop the run — Phase 2 has no stall-vs-failure distinction yet (Phase 4), so a non-green result is treated as a hard stop, not a skip-ahead
- [x] Reuse `retry.rs` backoff for transient gateway failures inside a phase (2026-07-28) — free: `start_fresh_and_record`'s existing `send_with_retry` (already used by human-initiated "Start Loop") already wraps every turn in `retry.rs`'s 529-overload backoff; reusing that function for phase turns means this item needed no new code
- [x] Make the queue resumable: on app restart, reload the plan and mark previously `running` phases `interrupted` for requeue (2026-07-28) — `run_executor::reconcile_running_phases`, called from `lib.rs`'s startup loop (mirroring the existing `conversation::reconcile_interrupted` per-project pass) and defensively again in `queue_run`/`get_run_status` when no in-memory `RunHandle` exists for the project
- [x] IPC commands `queue_run` / `cancel_run` / `get_run_status` plus TS wrappers and types (2026-07-28) — `commands/run_queue.rs` (registered in `lib.rs`); `cancel_run` fires the run's cancel flag *and* the project's existing interrupt slot (`commands::state::fire_interrupt`, extracted from `agent_interrupt` so both share one implementation) so cancellation doesn't wait for the in-flight turn to finish on its own. TS: `RunPlan`/`RunPhase`/`RunPhaseStatus`/`StallPolicy`/`PinnedAnswer`/`RunConsent`/`RunBudgets` in `types/index.ts`, `queueRun`/`cancelRun`/`getRunStatus` in `lib/tauri.ts`

### Phase 3 — Pre-flight interview

- [x] Add an interview pass that runs the orchestrator's clarify step for every queued phase before the run starts, while the user is present (2026-07-28) — `commands/run_queue.rs::run_phase_interview` drives one bounded turn per phase through the existing `start_fresh_and_record` pipeline (same question-card IPC surface chat already uses), built from `run_executor::build_interview_prompt`; the turn's `## Pre-flight Answers` closing block is parsed by `run_executor::extract_interview_answers` (last-occurrence, mirrors `extract_verdict`'s marker convention)
- [x] Pin interview answers into the `RunPlan` and inject them into each phase's session prompt (2026-07-28) — `run_phase_interview` writes the parsed answers to `RunPhase.interview` and saves the plan; injection into the phase's own turn was already wired in Phase 2's `build_phase_prompt`, which this phase's answers now actually populate
- [x] Block run start until every queued phase's interview is answered or explicitly skipped (2026-07-28) — new `RunPhase.interview_status: InterviewStatus` (`Pending` default / `Answered` / `Skipped`); `queue_run` refuses with a named-phase error while any `Queued` phase is still `Pending`; `skip_phase_interview` sets `Skipped` without running a session, for a phase the user judges unambiguous. No UI wiring yet — that's Phase 5's "Interview UI" item; this phase is IPC + persistence only (`run_phase_interview`/`skip_phase_interview` commands + TS wrappers/types)

### Phase 4 — Stall policy runtime

- [x] Detect mid-run stalls (question / permission / plan-approval cards) in the streaming event loop and park the phase instead of waiting (2026-07-28) — the executor now drives each phase through `commands::agent::start_fresh_and_record_streaming` (widened to `pub(crate)`, now returns `AgentResponse`) with a no-op sink `Channel`, since `claude_session.rs::answer_control_request` only parks a card when a channel is present — the non-streaming path (Phase 2/3) auto-denies instead. A new `AppError::TurnParked(String)` variant (distinct from `Agent`) is returned by `turn_deadline_expired_error` (now takes a `pending_detail` — the question text / `tool_name` + input / plan text, capped at 200 chars) from all three park sites (`answer_ask_user_question`, `answer_manual_permission`, `answer_plan_approval`) once `TURN_DEADLINE` (30 min) elapses unanswered — `commands/run_queue.rs::execute_run` matches on it ahead of the generic `Err(e)` arm to mark the phase `Parked` instead of `Failed`
- [x] Under `continue_independent`, continue with the next queued phase that has no dependency on a parked one; park dependents transitively (2026-07-28) — `run_executor::phases_blocked_by_park` (pure, unit-tested — transitive chain, unrelated phase stays queued, non-`Queued` phases ignored) computes the blocked set from `RunPhase.depends_on`; the executor loop needs no early-return branch since a policy that blocks nothing leaves the loop free to advance naturally
- [x] Implement the `halt` stall policy variant: a park halts all remaining phases, preserving strict sequence (2026-07-28) — `phases_blocked_by_park` returns every remaining `Queued` phase under `Halt`; once they're all marked `Parked` the loop's own top-of-iteration status check skips them, ending the run without a separate return path
- [x] Record every park with its pending question payload for the morning report (2026-07-28) — `RunPhase.park_payload` is set to the `TurnParked` detail for the phase that actually parked, and to `"blocked: depends on parked phase \"<id>\""` for its transitively-blocked dependents; `execution.yaml`'s `current` loop is abandoned with the same detail as the reason (not left dangling) so the next eligible phase, if any, can be promoted

  **Known ceiling, documented not fixed:** a stall is only detectable once it exceeds the existing 30-minute `TURN_DEADLINE` backstop — `claude_session.rs`'s park site isn't selected against any cancellation once entered (its own doc comment: "during a parked approval/question the loop is off the read, so an interrupt there won't be observed this turn"), so there is no way to notice or shorten a stall from outside `claude_session.rs` without a larger session-model change. That's out of this PRD's scope (sequencing/state, not environment/session architecture). In the common case this costs nothing: a human watching the app live sees the same pending card via the existing `agent_pending_question`/`_permission`/`_plan` surfaces (the slots are shared `AppState`, not tied to the run's no-op channel) and can answer it directly, so the turn completes normally and `TurnParked` is never reached.

### Phase 5 — Phase picker UI

- [x] Add phase multi-select and a "Queue overnight run" action to `EpicsPanel.tsx` (2026-08-01) — a `Square`/`CheckSquare` toggle button next to each not-done, stable-ID loop (`EpicsPanel.tsx`) feeds a selection array into the new `RunQueuePanel`; its picker bar (stall-policy `Select`, draft-PR-authorized checkbox, "Queue overnight run" button) calls the new `create_run_plan` command
- [x] Add a run-queue view showing live per-phase status (queued/running/parked/completed/failed/killed) via the existing streaming state (2026-08-01) — `RunQueuePanel.tsx` polls `get_run_status` every 5s and renders a status-colored badge + `park_payload` tooltip per phase, with "Start run"/"Cancel run" actions
- [x] Interview UI: present pre-flight questions as the existing question cards, gating the "Start run" action (2026-08-01) — needed no new question-card plumbing: `run_phase_interview` drives its turn through the same `start_fresh_and_record` pipeline as any other turn, so a parked `AskUserQuestion` lands in the same shared `AppState` slot `ProjectDetail.tsx`'s tab-agnostic `StuckQuestionCallout` already renders from, regardless of which tab is active. `RunQueuePanel` only needed "Answer" (awaits `run_phase_interview`) / "Skip" (`skip_phase_interview`) buttons per pending phase and a `canStart` gate requiring every `queued` phase's `interview_status !== "pending"`

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
