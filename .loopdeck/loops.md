# Loops

_Older loops archived to [loops-archive.md](./loops-archive.md)._

## Current

- **Status**: in_progress — `provider-contract` Phases 2–3 are implemented in worktree `run/provider-contract-normalize-claude-plus3-1aa529a93261`: both adapters dual-emit normalized `RunEvent`s, ts-rs bindings are CI-drift-checked, the run-queue consumer migrated, fixture/frontend contract tests pass, and the verify gate is PASS. Draft PR #110 is open.
- **Next**: review and merge draft PR #110; then start the next queued `execution-foundations` PRD.

## Next Steps
- [ ] Split oversized execution/session modules, especially commands/run_queue.rs, commands/agent.rs, claude_session.rs, codex_session.rs, and epic.rs.
- [ ] Smoke-test queued-loop continuation with live Codex/Claude after interruption; verify retained edits and shared state.
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

### 2026-10-30 — Codex sub-agent activity visibility
- **Status**: completed
- **Completed**: 2026-10-30
- **Summary**: Show collaboration call starts/results in live and persisted Worker activity through both event paths. Keep call completion separate from child status. Added event and display regression tests.
- **Validation**: All 36 frontend tests, TypeScript, formatting, and diff checks passed. Rust regression test is queued behind an existing Cargo build lock.
- **Follow-up**: Live smoke with delegated Codex work; child transcript subscription and reconciliation of authored loop status with provider lifecycle remain separate work.

### 2026-10-30 — Shared agent writing guidance
- **Status**: completed
- **Completed**: 2026-10-30
- **Summary**: Added shared ASD-STE100 writing guidance to Claude system instructions and the initial Codex prompt, including profiles without role charters.

### 2026-10-30 — Central queued-run state and unfinished-loop continuation
- **Status**: completed
- **Completed**: 2026-10-30
- **Summary**: Centralized run plans and execution snapshots in SQLite and added recovery-aware continuation with retained worktrees, provider checkpoints, and completed-phase preservation.

### 2026-10-28 — execution-foundations / prd-provider-contract Phases 2–3
- **Status**: completed
- **Completed**: 2026-10-28
- **Summary**: Both adapters now convert provider output at their boundary into the versioned `RunEvent` envelope and dual-emit it alongside the unchanged `ClaudeEvent` channel. Claude lines and Codex app-server messages map to started/content/tool/decision/usage/terminal/unsupported with a per-segment terminal gate, redaction, and `auto-allow` encoded as `allowed` + summary prefix. `ts-rs` generates `src/types/generated/` with a CI drift check; `useRunQueueEvents` migrated to the `run-event` topic through the pure `runEventProjection` reducer. Fixture parity tests (`provider_contract_tests.rs`) and reducer tests prove one terminal per segment, preserved approval payloads, and typed bindings. Full Rust suite, clippy, frontend build, and frontend tests pass.

### 2026-10-28 — Fix Claude control_response schema blocking unattended mutations
- **Status**: completed
- **Completed**: 2026-10-28
- **Summary**: The overnight provider-contract run could not write files: every mutating tool call failed with a harness `ZodError` because LoopDeck emitted `{"behavior":"allow"}` without `updatedInput`. `ControlResponsePayload::allow` now requires and serializes the original tool input, deny always carries a `message`, and regression tests cover both allow shapes. `cargo test` (692 passed), `cargo clippy -D warnings`, and `cargo fmt --check` are green. The parked run still needs a rebuild + re-queue.

### 2026-10-28 — execution-foundations / prd-provider-contract capture-fixtures
- **Status**: completed
- **Completed**: 2026-10-28
- **Summary**: Added sanitized Claude and Codex wire fixtures for text, tool use, approval/question, usage, error, interrupt, and successful completion under `src-tauri/tests/fixtures/provider-events/`, with a manifest mapping each scenario to both providers and its expected semantic payloads. Added `provider_fixture_tests.rs` to enforce JSON validity, Claude-parser compatibility, Codex envelope shape, payload coverage, one terminal event per terminating fragment, and secret/home-path cleanliness. Fixtures are hand-authored from the adapters' documented wire shapes (no live capture available); replace with sanitized live captures when possible. Also allowed `too_many_arguments` on `codex_session::turn_start_params` to keep `clippy -D warnings` green.

### 2026-10-14 — execution-foundations / prd-domain-contract Phase 1
- **Status**: completed
- **Completed**: 2026-10-14
- **Summary**: Added the state/ownership inventory, measured SQLite-vs-file decision and rollback procedure, and legacy identity/migration map; accepted SQLite as the target mutable operational store with compatibility exports.
