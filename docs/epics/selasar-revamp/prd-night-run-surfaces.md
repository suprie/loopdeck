---
prd: prd-night-run-surfaces
epic: selasar-revamp
milestone: "0.5.0"
status: proposed
description: >
  Reskin the existing overnight-run backend (`RunPlan`, `RunBudgets`,
  `RunReport`, `AuditSlice` from the 0.4.0 `overnight-orchestration` epic)
  onto the new detail drawer: a night variant with phase rail and budget
  gauges, a "Plan tonight" wizard, and a morning-report drawer — no new
  backend capability.
---

# PRD — Night-Run Surfaces

## Overview

Fourth and last PRD in the epic. `RunQueuePanel.tsx` and
`MultiAgentRuns.tsx` already implement the overnight-run UI logic this PRD
needs — phase status, parked-question parsing, budget display, requeue —
just in the current app's shape (mounted inside `EpicsPanel`/`AgentPanel`).
This PRD relocates and restyles that logic onto the drawer built in
`prd-detail-drawer`: a night variant of the drawer, a "Plan tonight" wizard
modal, and a morning-report drawer. It depends on that drawer existing,
since the night variant is a variant of the same component.

## Problem Statement

- The mockup's night-run visuals (phase-chip rail, budget gauges, parked
  inline card, wizard, morning report) are illustrated with fabricated
  example content — they must be rebuilt against the real
  `RunPlan`/`RunPhase`/`RunBudgets`/`RunReport`/`AuditSlice` types in
  `src-tauri/src/runplan.rs`, not re-derived from the mockup's HTML.
- The current run-queue UI lives inside `EpicsPanel.tsx` (queueing) and
  `AgentPanel.tsx` (`MultiAgentRuns`), separate from where a user would look
  under the new project-first IA — a project's own drawer.
- Whether "night run" is a new `RunState` enum value or a derived flag was
  left open by `prd-detail-drawer`'s spike; this PRD's Phase 1 needs that
  answer to build the drawer's variant-selection logic correctly.

## Goals

| Priority | Goal |
|----------|------|
| P0 | A night variant of the detail drawer shows a phase-chip rail (done/parked/current/queued) and three budget gauges (tokens, per-phase wall-clock, total run), sourced from the real `RunPlan` types. |
| P0 | An inline parked-question card with an "Answer & requeue" action, wired to the existing requeue IPC command. |
| P0 | A "Plan tonight" wizard (phase picker + stall policy + budgets → pre-flight interview → consent) reuses the existing interview/consent/queue-run IPC commands — no new backend endpoints. |
| P0 | A morning-report drawer (verdict table, parked questions, kill callouts, audit-log tail) sourced from the real `RunReport`/`AuditSlice` types. |
| P1 | The rail door and room card show a distinct "morning report ready" indicator that opens the report drawer. |

## Non-Goals

- Any new backend budget, consent, stall-policy, or run-plan capability —
  everything here reads/writes the existing `overnight-orchestration`
  IPC surface.
- Parallel phase execution, auto-merge, or any other capability the
  `overnight-orchestration` epic explicitly deferred — still deferred here.
- Redesigning the pre-flight interview or verify/verdict logic itself —
  this PRD only changes where and how the existing results are displayed.

## Design

_Stub — the exact mapping from `RunQueuePanel.tsx`'s current internal state
to the drawer's night variant is a Phase 1 output, once Phase 1 confirms
which parts of that component can be reused directly versus need
restructuring for the drawer's layout._

## Phases

### Phase 1 — Night drawer variant

- [x] Build the drawer's night variant (phase-chip rail + 3 budget gauges)
      sourced from the real `RunPlan`/`RunPhase`/`RunBudgets` types,
      reusing `RunQueuePanel.tsx`'s existing phase-row and
      parked-question-parsing logic rather than re-deriving shapes.
      (2026-08-26: `NightRunTab.tsx` + `src/lib/nightRun.ts`; status maps +
      parser relocated to a single shared source; `None` budget caps fall
      back to TS mirrors of `limits.rs` defaults per run clarification.)
- [x] Build the inline parked-question card (question text + "Answer &
      requeue" button) wired to the existing requeue IPC command already
      used by `RunQueuePanel.tsx`.
      (2026-08-26: `NightRunTab.tsx` parked-question inbox below the
      rail/gauges, per the run's pre-answered clarification — structured
      `__QUESTIONS__` payloads reuse the shared `AskUserQuestionCard` with
      submit relabeled "Answer & requeue" → `answerParkedQuestion`; raw
      payloads get a plain "Answer & requeue" button → `requeueRunPhase` +
      `queueRun`, the exact `RunQueuePanel` Retry flow. New
      `parkedInbox(plan)` in `lib/nightRun.ts` status-gates the cards.)
- [x] Wire the rail door's night-run indicator and the drawer's variant
      selection to switch to this variant automatically when a project has
      an active `RunPlan`, per `prd-detail-drawer`'s spike decision on how
      "night run" is represented.
      (2026-08-26: spike ADR-3 confirmed the rail's placeholder-derived
      `hasActiveOrQueuedRun` flag as *the* representation — no new
      `RunState` variant — so the door badge stays as built (comments
      de-placeholdered); the missing half was drawer auto-selection:
      `ProjectDrawer` now auto-selects the Agent tab (which swaps to
      `NightRunTab`) once per continuous drawer-open span per project via
      new `shouldAutoSelectNightVariant` in `lib/nightRun.ts` — a user who
      navigates away mid-run is never yanked back.)

### Phase 2 — Plan-tonight wizard

- [x] Build the 3-step wizard (phase picker with dependency labels + stall
      policy toggle + budget inputs → pre-flight interview text inputs with
      skip checkboxes → consent summary + required checkbox), reusing the
      existing pre-flight-interview and queue-time-consent IPC commands.
      (2026-08-28: `PlanTonightWizard.tsx` mounted from the drawer header's
      `PlanTonightButton`. Per the run's pre-answered clarifications:
      dependency labels come from new tested `dependencyLabel()` in
      `lib/nightRun.ts` (mirrors `build_run_plan`'s authored-order
      predecessor chain); `createRunPlan` fires on the 1→2 transition with
      draft-PR consent pre-checked; step 2 runs live interviews inline
      (Run-interview/Skip controls — parked `AskUserQuestion` cards render
      inline by polling the shared pending-question slot, answer fields are
      the "text inputs"); step 3's required checkbox gates only the final
      action.)
- [x] Wire the wizard's final action to the existing queue-run command,
      confirming the phase/budget/consent payload shape matches what
      `run_executor.rs` expects.
      (2026-08-28: Start button calls the same `queueRun` IPC
      RunQueuePanel's Start uses. Payload-shape confirmation: `queue_run`
      takes only the project path and re-reads `run-plan.yaml` — the
      phase/budget/consent shape was fixed by `createRunPlan` at the 1→2
      transition, so no new payload crosses the boundary. Button gating
      mirrors `queue_run`'s pending-interview guard; on resolve the wizard
      closes and auto-switches to the Agent tab, which the runStatus poll
      swaps for the night variant.)
- [x] Add the "Plan tonight" entry point to the drawer header, gated on the
      project having queueable PRD phases (mirroring whatever gate
      `EpicsPanel.tsx` currently uses).
      (2026-08-26: `PlanTonightButton` in `ProjectDrawer.tsx`'s header, gated
      by new `hasQueueablePhases(epics)` in `lib/nightRun.ts` — the picker
      checkbox's exact `!done && !noId` condition as a shared tested helper.
      Per the run's pre-answered clarification, the button holds local
      open/close state only until items 1-2's wizard exists.)

### Phase 3 — Morning report drawer

- [ ] Build the morning-report drawer (verdict table, parked-questions
      section reusing Phase 1's inline card, kill callout rows, collapsible
      audit-log tail) sourced from the real `RunReport`/`AuditSlice` types,
      reusing `RunQueuePanel.tsx`'s existing morning-report rendering where
      possible.
- [ ] Wire the room card's/rail door's "morning report ready" indicator to
      open this drawer.
- [ ] Wire the report's "Answer & requeue" button to the same requeue
      command as the night variant's inline parked card.

### Phase 4 — Verification

- [ ] Manual smoke test against a real queued run: plan tonight, let (or
      simulate) a phase complete, confirm the morning report shows verdicts/
      parked/kills matching the actual `RunReport`.
- [ ] `npx tsc --noEmit` clean; `cd src-tauri && cargo test` clean, confirming
      no accidental backend edits broke existing `runplan`/`run_executor`
      tests.

## Open Questions

- Once this PRD relocates `RunQueuePanel.tsx`'s logic into the drawer, does
  the old mount point inside `EpicsPanel.tsx` get deleted outright, or does
  it stay as a fallback until the drawer ships? Resolve in Phase 1 — default
  to deleting it once the drawer variant is confirmed working, per this
  repo's "no compatibility shims" convention.
- Should the "morning report ready" indicator auto-clear once the report's
  been opened once, or stay until every parked question in it is resolved?
  Resolve in Phase 3.
