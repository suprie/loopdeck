# Decisions

_Older decisions archived to [decisions-archive.md](./decisions-archive.md)._ 

## 2026-10-28 — Dual-emit normalized provider events during migration
- **Status**: accepted
- **Context**: `prd-provider-contract` Phase 2 must normalize Claude and Codex output without breaking existing `ClaudeEvent` consumers before the replay migration.
- **Consequences**: Both adapters keep their legacy `Channel<ClaudeEvent>` payload byte-compatible and additionally feed `provider_normalize::RunEventNormalizer`; the detached run queue re-broadcasts the normalized stream on the additive `run-event` topic. A per-segment terminal gate guarantees one durable terminal, retries are new segments with transient `retrying` signals, and capability gaps become explicit `unsupported` events. Interactive commands and multi-agent workers do not attach a normalized stream yet.

## 2026-10-28 — ts-rs owns the run-event TypeScript bindings
- **Status**: accepted
- **Context**: P1 requires generated or mechanically validated TypeScript bindings, and hand-maintained event shapes drift from Rust.
- **Consequences**: `provider_event.rs` derives `ts_rs::TS`; `cargo test export_bindings` writes `src/types/generated/`, and CI regenerates then requires a clean `git diff --exit-code`. Token counts and the sequence placeholder are generated as `number` (not `bigint`). Decision render details travel in an additive `detail_redacted` JSON field so approval/question/plan payloads are not lost; automatic approval stays `DecisionState::Allowed` with an `auto-allow: ` summary prefix rather than a new enum variant.

## 2026-10-28 — Every Claude control_response allow must echo updatedInput
- **Status**: accepted
- **Context**: The unattended provider-contract run stalled because every mutating tool call failed before execution with `Tool permission request failed: ZodError ... updatedInput ... / behavior "deny"`. LoopDeck wrote a bare `{"behavior":"allow"}` for policy auto-approvals and plan approvals; the Claude CLI's response schema now requires a record `updatedInput` on every allow (and a string `message` on every deny).
- **Consequences**: `ControlResponsePayload::allow` now takes the original tool input and always serializes `updatedInput`; permission, manual-approval, plan-approval, and AskUserQuestion paths all pass it through. `deny` always serializes a `message` string. Regression tests cover plain allow and rewritten-input allow. This unblocks unattended runs once the app is rebuilt and the parked run is re-queued.

## 2026-10-28 — Provider fixtures are manifest-backed and synthetic until sanitized captures exist
- **Status**: accepted
- **Context**: `prd-provider-contract` Phase 1 needs both providers' wire shapes for text, tools, decisions, usage, error, interrupt, and completion before normalization can be tested, but no live account capture was available in this environment.
- **Consequences**: Added hand-authored sanitized fixtures under `src-tauri/tests/fixtures/provider-events/` with a `manifest.json` mapping each scenario to both provider files, expected semantic payloads, and the terminal outcome. `provider_fixture_tests.rs` enforces JSON validity, Claude-parser compatibility, Codex envelope shape, payload coverage, exactly one terminal event per terminating fragment, and secret/home-path cleanliness. Replace them with sanitized live captures when available without changing the manifest shape.

## 2026-10-14 — Provider events use a versioned semantic envelope
- **Status**: accepted
- **Context**: Claude and Codex need a shared durable event meaning before replay and persistence migration.
- **Consequences**: Added version-1 `RunEvent` with explicit capability gaps, redacted durable fields, diagnostic-only raw references, and exactly-one terminal rules; legacy provider events remain until normalization.

## 2026-10-14 — SQLite is the target operational store with compatibility exports
- **Status**: accepted
- **Context**: `prd-domain-contract` Phase 1 inventory found mutable run, phase, decision, session, and delivery state split across YAML, JSONL, manifests, and `AppState`; the canonical fixture is small, but cross-file atomicity and restart reconciliation are the limiting constraints.
- **Consequences**: Later persistence work may implement SQLite for mutable operational state, but must preserve authored repository files, import provenance, compatibility exports, backup/restore, and a visible divergence gate. The legacy-to-new identity mapping and irrecoverable in-memory cases are documented in `docs/epics/execution-foundations/migration-map.md`.

## 2026-10-14 — Transition edges are pure and terminal attempts are immutable
- **Status**: accepted
- **Context**: Phase 2 needs one transition contract across interactive, queued, multi-agent, decision, and delivery state before persistence migration.
- **Consequences**: `src-tauri/src/transition.rs` owns allowed edges and structured invalid-transition errors; restart reconciliation now routes `running → interrupted` through it, while retries create new attempts.

## 2026-10-08 — Keep Codex integration fixtures aligned with Code Mode transport
- **Status**: accepted
- **Context**: PR CI failed because the charter-injection fixture still published the retired WebSocket endpoint after production Code Mode moved to local gRPC/HTTP endpoint validation; strict Clippy also caught an unnecessary borrow.
- **Consequences**: The fixture now publishes a local gRPC endpoint and the launcher uses the clippy-clean `StdCommand::new(binary)` form. Full serialized Rust tests pass.

## 2026-10-08 — Track Rust Atomic API renames
- **Status**: accepted
- **Context**: CI treats deprecations as errors, and the current Rust toolchain renamed `Atomic<u64>::fetch_update` to `try_update`.
- **Consequences**: Token-budget accounting now uses `try_update` while preserving the existing saturating increment behavior.

## 2026-10-08 — Keep Codex handshake fixtures current
- **Status**: accepted
- **Context**: The Codex charter-injection fixture stopped before `turn/start` because production now queries collaboration modes and models during initialization.
- **Consequences**: The fake app-server answers `collaborationMode/list` and `model/list` alongside the existing handshake methods.

## 2026-10-08 — Box provider sessions in the harness enum
- **Status**: accepted
- **Context**: Strict Clippy rejected `HarnessSession` because concrete Claude and Codex session variants have materially different sizes.
- **Consequences**: Both provider sessions are heap-allocated inside `HarnessSession`, reducing enum size while preserving the existing provider delegation API.

## 2026-10-08 — Separate orchestrator and worker language in run surfaces
- **Status**: accepted
- **Context**: The night-run UI labeled the orchestration card and the Codex worker transcript both as “Agent,” making a managed worktree worker look like the project’s interactive agent and making stale activity difficult to interpret.
- **Consequences**: The detail tab is now “Worker activity”; the night-run card identifies orchestrator status, worker profile, managed-worktree isolation, and current phase; the transcript surface explicitly identifies itself as worker activity.

## 2026-10-09 — Correlate Codex turn failures across the run watchdog
- **Status**: accepted
- **Context**: An unattended run ended with a generic “wedged app-server” message after the Codex rollout stopped during a patch call; the existing logs could not distinguish a tool hang, protocol failure, watchdog timeout, or cleanup failure.
- **Consequences**: Codex sessions now log turn/request IDs, item start/completion, interrupt lifecycle, completion status, and terminal errors; the run queue logs watchdog budgets, worktree/run context, and whether a cached session existed when it was removed. Prompts and full tool arguments remain excluded from the new lifecycle logs.

## 2026-10-10 — Use write-capable Codex policy for unattended runs
- **Status**: accepted
- **Context**: Unattended Codex turns used read-only sandboxing with on-request approval, so Cargo could wait for a host popup that the worker UI could not show.
- **Consequences**: Autonomous turns use `workspaceWrite` with `approvalPolicy: on-request`; interactive turns retain read-only approval cards, while all requests remain subject to LoopDeck's destructive-command floor.

## 2026-10-30 — Central operational snapshots and queued-loop continuation
- **Status**: accepted
- **Context**: Interrupted queued runs lost execution continuity, and worktree-local YAML could diverge.
- **Consequences**: Run plans and execution snapshots now use per-user SQLite; continuation retains worktrees, checkpoints, diagnostics, and completed phases.
- **Detail**: docs/epics/execution-foundations/operational-recovery.md

## 2026-10-30 — Shared ASD-STE100 writing guidance
- **Status**: accepted
- **Context**: Agent explanations should use short sentences and consistent terms.
- **Consequences**: Claude and Codex receive shared writing guidance at session creation; code, literal content, and required output formats remain unchanged.

## 2026-10-30 — Surface Codex delegation as worker activity
- **Status**: accepted
- **Context**: Codex collaboration items were ignored, leaving a busy parent session with no visible delegated activity.
- **Consequences**: Both event paths and persisted transcripts retain collaboration call starts/results, model, child IDs, prompts, and reported child states. A completed collaboration call does not imply a completed child task or parent turn. Child transcripts are not subscribed by this change.

## 2026-10-30 — Edit budgets on stopped run plans
- **Status**: accepted
- **Context**: A phase stopped by its token or time limit needs a higher budget without replacing the run plan.
- **Consequences**: A typed IPC command validates and saves budget changes only for inactive runs. Phase outcomes, provider sessions, and retained worktrees remain available to the existing continuation path.
