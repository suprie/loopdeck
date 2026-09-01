# Loops

_Older loops archived to [loops-archive.md](./loops-archive.md)._

## Current

- **Started**: 2026-09-01
- **Goal**: `role-based-orchestration` / `prd-role-foundations` Phase 1 — complete. Two queued loops in one unattended session: (1) `charter-model` — new `RoleCharter` (`config.rs`: optional `persona_prompt`, `allowed_skills`, `output_contract`) `#[serde(flatten)]`ed onto `NamedAgentConfig` beside the flattened `AgentConfig`; old `config.yaml` without charter keys loads unchanged (empty charter = plain connection profile); `update_agent_config` preserves the charter on connection edits; `normalized()` trims prose/skills and collapses all-empty back to default so cleared fields vanish from YAML. (2) `charter-crud` — new IPC `update_agent_charter` (registered in `lib.rs`, in-memory rollback, no secrets) + `GlobalConfig::update_agent_charter` (replace-all, UUID/name checked); frontend: `RoleCharter` type, roster-client `updateCharter`, `updateAgentCharter` wrapper, "Role charter (optional)" section in `AgentConfigEditor` (persona + output-contract textareas, comma-separated skills), `AgentRoster.save` issues the charter call after create (skipped when empty) / update (replace-all clears). Tests: 3 Rust charter tests (legacy YAML load, roundtrip+normalize, replace/clear/preserve-on-connection-edit), +1 frontend contract test (charter replace + preserved across connection update + omit-clears). Gates: `cargo test` 643 passed, clippy clean, `tsc --noEmit` clean, frontend tests 23/23.
- **Status**: completed

## Next Steps
- [ ] Review & merge: https://github.com/suprie/loopdeck/pull/101
- [ ] Review & merge the draft PR for this run (see final chat message for URL)
- [ ] Review & merge the night-run Phase 3 draft PR: https://github.com/suprie/loopdeck/pull/92
- [ ] `prd-night-run-surfaces` Phase 4 manual smoke: real queued run planned via the wizard, through the night variant, into the morning report (verdicts/parked/kills match the actual `RunReport`)
- [ ] Human smoke of `DeliveryReportTab` (RetryCard/HandoffBanner) in the running app (Tauri webview not drivable headless)

## History

### 2026-09-01 — prd-verified-delivery-reconciliation Phases 4-5 (clean-handoff, retry-recovery, delivery-integration-tests, prd-acceptance-audit)
- **Status**: completed
- **Completed**: 2026-09-01
- **Summary**: handoff.rs + delivery_retry.rs + stub-gh integration tests + acceptance audit; all 6 P0 goals PASS. Detail: decisions.md 2026-08-31 / 2026-09-01 entries.

### 2026-08-31 — prd-verified-delivery-reconciliation Phases 1-3
- **Status**: completed
- **Completed**: 2026-08-31
- **Summary**: Seven loops in one unattended session — `delivery.rs` (DeliveryLinks + pure reconcile/gates + rubric extraction), `DeliveryReportTab` + report/rubric commands, worktrees under `.loopdeck/runs/` with resume-from-branch, external-worktree detection (detect-only), executor delivery gates on a fresh rubric, rubric summary in the delivery commit, checklist completion only after the PR exists. Detail: decisions.md 2026-08-31 entries.

- ✅ **`selasar-revamp` / `prd-night-run-surfaces` Phase 3 — morning report, rail badge, requeue** (2026-08-30) — `MorningReportTab.tsx` (verdict table + kill callouts + collapsible `AuditSlice` tail, Agent-tab slot), the rail door's sun "morning report ready" badge (clear-once-opened via `appStore.morningReportSeen`), and the report's parked-questions requeue via the shared `ParkedQuestionInbox.tsx` extracted from `NightRunTab.tsx` (both paths: `answerParkedQuestion` / `requeueRunPhase`+`queueRun`; stay-on-report latch + 5s report refetch). Draft PR #92.

- ✅ **`prd-night-run-surfaces` Phase 2 items 1-2 — plan-tonight wizard + queue-run wiring** (2026-08-28) — Branch started by merging draft PRs #87 + #88 (run's pre-answered clarification) so the wizard's mount point (`PlanTonightButton`, item 3) and Phase 1's night variant exist. Item 1: new `PlanTonightWizard.tsx` — a 3-step shadcn-`Dialog` opened from the drawer header. Step 1: queueable-loop picker (same `!done && !noId` gate, grouped by epic · PRD · phase) where each checked loop shows its dependency label from new pure `dependencyLabel()` (`lib/nightRun.ts`, mirrors `run_executor.rs::build_run_plan`'s authored-order predecessor chain — selection order IS the dependency chain), plus stall-policy select, budget inputs, and draft-PR consent checkbox. `createRunPlan` fires on the 1→2 transition (clarification: interviews need a plan; abandonment leaves it queued-but-unstarted, same state the panel leaves). Step 2: live pre-flight interviews inline — per-phase Run-interview/Skip controls (same IPC as `RunQueuePanel`'s rows); a running interview polls `listPendingQuestions` at 1s and renders a parked `AskUserQuestionCard` inline below the row (the "text inputs"); submitting answers via `agentAnswerQuestion` unblocks the turn — `runPhaseInterview` then resolves with answers pinned (clarification named `answerParkedQuestion`, which only applies to phases parked mid-run, i.e. `status === "parked"`; a live interview parks on the shared pending-question slot, whose answer path is `agentAnswerQuestion` — deviation recorded in decisions.md). Step 3: consent summary (phases + interview statuses, stall policy, budgets, draft-PR consent) + required checkbox. Item 2: Start button calls the same `queueRun` IPC `RunQueuePanel`'s Start uses, with `useStreamingState` begin/clear + toasts; payload-shape confirmation — `queue_run` takes only the project path and re-reads `run-plan.yaml` (shape fixed by `createRunPlan`), and button gating mirrors `queue_run`'s pending-interview guard; on resolve the wizard closes and auto-switches to the Agent tab (clarification: "close wizard, auto-switch"), which the `useRunStatus` poll swaps for the night variant. Split honored per clarification: loop 1 committed with a disabled stub final action (`650c1c3`), loop 2 replaced it with the wiring (`da093c9`). Tests: +1 `dependencyLabel` case in `tests/frontend/nightRun.test.mjs` (first-phase no-deps, predecessor chain, reorder re-chains, raw-ID fallback). Gates green: `npx tsc --noEmit`, `npm run test:frontend` 20/20, `cargo test` 596 passed, `clippy` clean. Verifier PASS (2/2 criteria, no non-goals scope creep).

- ⛔ **`prd-night-run-surfaces` Phase 2 item 2 — wire the wizard's final action to queue-run: BLOCKED, zero code** (2026-08-26) — Dependency-order mismatch: the run plan queues this loop before `build-the-3-step-wizard`, reversed from the PRD's Phase 2 listing, so at execution time no wizard exists — only item 3's entry-point stub (`PlanTonightButton`, local open/close state, documented as the wizard's future mount point at `ProjectDrawer.tsx:491`). Pre-answered clarification for exactly this case: treat as blocked, record the mismatch as a parked question, produce no code against an undefined target. `loopdeck-prd-verifier` against the loop's criterion: **BLOCK** — FAIL (no wizard, no final action, no queue-run wiring in the changed set; the only `queueRun` call sites are `RunQueuePanel`'s pre-existing retry flow and `NightRunTab`'s Phase 1 requeue, neither wizard-related). Non-goals audit clean. No PR opened (BLOCK verdict gates `open-pr`); parked question in Next Steps for a human to reorder `run-plan.yaml`. Superseded 2026-08-28: run plan reordered, both loops re-queued and completed (entry above).
- ⛔ **`prd-night-run-surfaces` Phase 2 item 2 — wire the wizard's final action to queue-run: BLOCKED, zero code** (2026-08-26) — Dependency-order mismatch: the run plan queues this loop before `build-the-3-step-wizard`, reversed from the PRD's Phase 2 listing, so at execution time no wizard exists — only item 3's entry-point stub (`PlanTonightButton`, local open/close state, documented as the wizard's future mount point at `ProjectDrawer.tsx:491`). Pre-answered clarification for exactly this case: treat as blocked, record the mismatch as a parked question, produce no code against an undefined target. `loopdeck-prd-verifier` against the loop's criterion: **BLOCK** — FAIL (no wizard, no final action, no queue-run wiring in the changed set; the only `queueRun` call sites are `RunQueuePanel`'s pre-existing retry flow and `NightRunTab`'s Phase 1 requeue, neither wizard-related). Non-goals audit clean. No PR opened (BLOCK verdict gates `open-pr`); parked question in Next Steps for a human to reorder `run-plan.yaml`.

- ✅ **`prd-night-run-surfaces` Phase 2 item 3 — "Plan tonight" entry point in the drawer header** (2026-08-26) — `ProjectDrawer.tsx` gains a local `PlanTonightButton` in the `SheetHeader` (left of the status badge): fetches epics via `api.getEpics` on mount (runs only while the drawer is open — Radix `Sheet` mounts content conditionally), renders null when nothing is queueable or the fetch fails. Gate is new pure `hasQueueablePhases(epics)` (`lib/nightRun.ts`), mirroring `EpicsPanel.tsx`'s per-loop overnight-run picker gate verbatim (`loop.id && !loop.checked && !loop.done_in_history`, the `!done && !noId` condition at EpicsPanel.tsx:733-752) — shared single source rather than an inline copy, so the header button and the picker checkboxes can't drift. Per the run's pre-answered clarification (wizard items 1-2 queued separately), the button toggles local `open` state (`aria-expanded`, primary-highlighted when open) with no modal content yet — the wizard mounts into that open state later. Tests: +1 `hasQueueablePhases` case in `tests/frontend/nightRun.test.mjs` (open-ID'd loop queueable, id-less/checked/done-in-history not, any-qualifier-anywhere, empty tree). Verifier PASS (1/1 criterion, no non-goals scope creep).

- [ ] Review & merge the memory-hygiene draft PR: https://github.com/suprie/loopdeck/pull/94

- [ ] `prd-night-run-surfaces` Phase 4 manual smoke: real queued run planned via the wizard, through to the morning report
- [ ] `prd-process-discipline` owns automated budget enforcement if ever wanted (resolved: document-only here)

## History

### 2026-08-30 — Memory hygiene: 3,000-token budget + compaction
- **Status**: completed
- **Completed**: 2026-08-30
- **Summary**: Measured loops.md ~40K / decisions.md ~8.2K tokens, wrote the 3,000-token budget and entry-length convention into `loopdeck-memory`, compacted both active files to ~0.8K/~2.1K tokens, verified the archives. Detail: `memory-budget-report.md`; `loops-archive.md` 2026-08-30 appendix.

### 2026-08-30 — prd-night-run-surfaces Phase 3 — morning report drawer
- **Status**: completed
- **Completed**: 2026-08-30
- **Summary**: Morning report renders in the Agent-tab slot with a stay-on-report latch; parked inbox single-sourced. Shipped as PR #92 (merged). Detail: `loops-archive.md`.

### 2026-08-28 — Plan-tonight wizard: 3-step phase picker with dependency labels
- **Status**: completed
- **Completed**: 2026-08-28
- **Summary**: Wizard's 3-step picker renders phase dependency labels and unblocks stall recovery. Detail: `loops-archive.md` 2026-08-30 appendix.

### 2026-08-26 — prd-night-run-surfaces Phase 1 items 1-3
- **Status**: completed
- **Completed**: 2026-08-26
- **Summary**: Night drawer variant, inline parked-question card, rail-door indicator delivered. Detail: `loops-archive.md` 2026-08-30 appendix.

### 2026-08-23 — prd-detail-drawer Phase 1 (spike) + Phase 2 (overlay drawer)
- **Status**: completed
- **Completed**: 2026-08-23
- **Summary**: Drawer is pure UI state; Epics/Graph nest under Loops/Decisions. Detail: `loops-archive.md` 2026-08-30 appendix.

### 2026-08-12 — prd-rail-corridor-shell Phase 1 — project rail
- **Status**: completed
- **Completed**: 2026-08-12
- **Summary**: Project rail with all 4 loop-domain doors. Detail: `loops-archive.md` 2026-08-30 appendix.
