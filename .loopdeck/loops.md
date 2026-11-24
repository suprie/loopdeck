# Loops

_Older loops archived to [loops-archive.md](./loops-archive.md)._

## Current

- **Status**: in_progress — all six `prd-parallel-scheduling` loops are implemented in worktree `run/parallel-scheduling-one-turn-per-phase-plus5-d13a619fe69b`: per-phase turns, dependency-aware fan-out with per-phase worktrees, the `Waiting` gate, multi-phase restart reconciliation, per-phase budgets, and scheduler property/worktree tests. Verify gate is PASS; PR #113 was merged.
- **Next**: Run a real overnight plan with two disjoint phases to confirm concurrent draft PRs on the user's machine.

## Next Steps
- [ ] Split oversized execution/session modules, especially commands/run_queue.rs, commands/agent.rs, claude_session.rs, codex_session.rs, epic.rs, and run_executor.rs.
- [ ] Smoke-test queued-loop continuation with live Codex/Claude after interruption; verify retained edits and shared state.
- [x] Repair draft delivery after the phase watchdog and rerun both independent phases to completion.
- [x] Address PR #113 review findings for planning, phase isolation, restart recovery, budgets, and delivery retries.
- [x] Resolve PR #113 against agent-handoff support on main; preserve bounded injection, citation attribution, and waiting-phase restart recovery.
- [x] Review & merge: https://github.com/suprie/loopdeck/pull/113
- [x] `role-based-orchestration` / `prd-parallel-scheduling` — all six loops (one-turn-per-phase, parallel-batches, gated-start, restart-reconcile, budget-split, tests).
- [x] Fix the Claude control_response schema and commit it.
- [x] Create a fresh run worktree and plan from `feat/provider-event-fixtures`.
- [x] `execution-foundations` / `prd-provider-contract` Phase 2 — normalize Claude and Codex provider output into the versioned `RunEvent` envelope.
- [x] `execution-foundations` / `prd-provider-contract` Phase 3 — fixture-based contract tests plus the migrated consumer's projection test.
- [ ] Review & merge draft PR #110: https://github.com/suprie/loopdeck/pull/110
- [ ] Review & merge: https://github.com/suprie/loopdeck/pull/109
- [ ] Re-run a single overnight phase with `loopdeck::codex_lifecycle` and `loopdeck::run_queue` logs enabled; compare the last lifecycle event with the watchdog decision.
- [ ] Run the overnight pre-flight smoke again after the Codex handshake fix
- [ ] Review and publish the new beta corporate site in `website/`
- [ ] Confirm the generated product screenshot’s text artifacts are acceptable for public beta branding
- [ ] Review the transparent product visual on the dark website background
- [ ] Review the transparent-corner Selasar icon at small sizes
- [ ] Review & merge: https://github.com/suprie/loopdeck/pull/102
- [ ] Review & merge the draft PR for this run (see final chat message for URL)
- [ ] Review & merge the night-run Phase 3 draft PR: https://github.com/suprie/loopdeck/pull/92
- [ ] `prd-night-run-surfaces` Phase 4 manual smoke: real queued run planned via the wizard, through the night variant, into the morning report (verdicts/parked/kills match the actual `RunReport`)
- [ ] Human smoke of `DeliveryReportTab` (RetryCard/HandoffBanner) in the running app (Tauri webview not drivable headless)

## History

### 2026-11-24 — Harden operational state import and recovery
- **Status**: completed
- **Completed**: 2026-11-24
- **Summary**: Preserved malformed SQLite execution records for diagnosis and made legacy run-plan import persist the same data it validates.

### 2026-11-24 — Integrate run recovery with parallel scheduling
- **Status**: completed
- **Completed**: 2026-11-24
- **Summary**: Resolved PR #115 against the per-phase scheduler and carried forward SQLite state, session recovery, and editable budgets.

### 2026-11-24 — Address orchestration recovery review
- **Status**: completed
- **Completed**: 2026-11-24
- **Summary**: Isolated nested projects, serialized budget edits and requeueing, preserved provider checkpoints when regrouping phases, and carried saved wall-clock usage forward on continuation.

### 2026-10-30 — Prepare orchestration recovery PR
- **Status**: completed
- **Completed**: 2026-10-30
- **Summary**: Packaged shared SQLite operational state, interrupted-loop continuation, editable budgets, sub-agent activity, and shared writing guidance. Integrated the merged handoff implementation before final verification. Runtime execution and delivery snapshots remain local.

### 2026-10-30 — Adjust stopped-run budgets
- **Status**: completed
- **Completed**: 2026-10-30
- Added an existing-run budget editor for token and time limits; save limits before continuing unfinished phases. Active runs reject budget updates.
- Validation: TypeScript type-check passed; backend budget tests recorded in session results.
- Follow-up: smoke-test a real resource-limit stop, budget increase, and continuation in the desktop app.
