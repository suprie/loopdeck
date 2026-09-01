# Decisions

_Older decisions archived to [decisions-archive.md](./decisions-archive.md)._

## 2026-08-30 — Night-run status supports, rather than replaces, the agent conversation
- **Status**: accepted
- **Context**: The night-run Agent tab replaced the chat transcript with phases and budgets, and answering a structured parked question requeued it without restarting execution.
- **Consequences**: The Agent tab now keeps chat mounted with a compact, collapsible run-status companion; Answer starts the requeued run so agent activity resumes visibly.



## 2026-08-30 — Multi-agent detail is on demand
- **Status**: accepted
- **Context**: The launcher and historical sub-run cards occupied much of the Agent tab even when the user only needed the conversation.
- **Consequences**: A compact agent-name row now opens each sub-run in a popover, and profile selection plus launch live in a separate Run popover.



## 2026-08-30 — Draft-PR delivery is terminal even when its PRD ID drifts
- **Status**: accepted
- **Context**: An agent could verify PASS and create a draft PR after changing/removing the queued stable ID, causing the completed work to look unmatched and tempting a costly rerun.
- **Consequences**: Verification auto-relinks only an exact epic/PRD/phase/title match; otherwise the phase remains non-retryable `delivered` with PR evidence and a guided fallback relink.


## 2026-08-30 — Memory files carry a 3,000-token budget; the token budget supersedes count windows
- **Status**: accepted
- **Context**: `loops.md` had regrown to ~40K tokens (chars/4), recreating the 2026-07-19 runaway-re-read incident the 90KB archive fix meant to prevent; entry length, not entry count, dominates cost.
- **Consequences**: `loopdeck-memory` now pins a 3,000-token/file budget, a 2,400-token archive trigger, ≤60-word decisions entries and ≤50-word loop summaries (chars/4, document-only enforcement); both active files compacted under budget, archives deduped with verified pointers.
- **Detail**: `.loopdeck/memory-budget-report.md`

## 2026-08-29 — Chat support code is isolated by responsibility
- **Status**: accepted
- **Context**: `Chat.tsx` mixed chat orchestration with reusable formatting, transcript normalization, and both parked-approval controls, making a high-churn surface hard to scan safely.
- **Consequences**: Moved pure chat helpers into `chatUtils.ts` and approval UI into `ApprovalCards.tsx`; `Chat.tsx` preserves its existing public exports as re-exports and remains the composer/transcript coordinator.



## 2026-08-29 — Harness parking contract is separate from the Claude process
- **Status**: accepted
- **Context**: The Claude and Codex harnesses both depend on the pending-question, permission, plan, and interrupt-slot types, but they were defined inside the 2,800-line Claude session implementation.
- **Consequences**: Moved that shared contract to `claude_session/parking.rs` and re-exported it through `claude_session`, leaving downstream imports and runtime behavior unchanged.



## 2026-08-28 — Multi-agent run cards bounded and collapsed; the conversation keeps the panel
- **Status**: accepted
- **Context**: Finished multi-agent runs stacked as full-height cards in the Agent panel (the section was `shrink-0` and unbounded), squeezing the conversation transcript into an unreadable strip.
- **Consequences**: The `MultiAgentRuns` section is capped at 45% of the panel column with an internal scroll area, and terminal runs (done/failed/cancelled) collapse to one compact row (agent names + status + relative time + one-line result/error, click to expand); non-terminal runs stay expanded.


## 2026-08-28 — Streaming-fresh pipeline converts `is_error` responses to `Err` (429 no longer shows as completed)
- **Status**: accepted
- **Context**: `start_fresh_and_record_streaming_in_root_with_config` returned `Ok` for CLI-level errors (e.g. 429 rate limits) even though the non-streaming `send_and_record` converts them, so multi-agent manifests and the run queue persisted failed runs as done.
- **Consequences**: The streaming-fresh funnel now mirrors `send_and_record` — records the assistant turn, then returns `Err(AppError::Agent(…))` on `is_error` — so multi-agent sub-runs persist as `failed` and run-queue phases stop reporting fake completions; `.loopdeck/agent-runs/` added to `.gitignore`.

## 2026-08-28 — Plan-tonight wizard: createRunPlan at the 1→2 transition; live-interview parks answer via the shared slot, not answerParkedQuestion
- **Status**: accepted
- **Context**: `prd-night-run-surfaces` Phase 2's wizard needed `createRunPlan` (which takes queue-time draft-PR consent) before its step 2 could run interviews, yet the consent UX sits at step 3 — a circular ordering. Separately, the run's pre-answered clarification said a parked interview question "answers pin via `answerParkedQuestion`", but that command requires `phase.status === "parked"` (mid-run park), while a *pre-flight* interview phase stays `queued` and parks its `AskUserQuestion` on the shared pending-question slot that chat's callouts already consume.
- **Consequences**: `createRunPlan` fires on the wizard's step 1→2 transition with the draft-PR consent checkbox pre-checked (matching `RunQueuePanel`'s default); step 3's required checkbox gates only the final `queueRun` action. Abandoning the wizard mid-way leaves the plan queued-but-unstarted — the same state the existing panel leaves, and re-entering replaces it (`createRunPlan`'s existing replace semantics, no new backend behavior). For parked interview questions, the wizard polls `listPendingQuestions` at 1s while a turn runs, renders the shared `AskUserQuestionCard` inline, and submits via `agentAnswerQuestion` — the slot's answer path — which unblocks the turn so `runPhaseInterview` resolves with the answers pinned. Documented deviation from the clarification's literal wording, honoring its intent (inline card, pinned answers) with the command that actually resolves the state it finds. The wizard's final action passes only the project path to `queueRun`, which re-reads `run-plan.yaml` — so the phase/budget/consent payload shape is exactly what `createRunPlan` (and through it `build_run_plan`) wrote, with no frontend-reconstructed payload to drift.


## 2026-08-27 — Pending agent questions render collapsed; answer-IPC failure clears sub-run cards
- **Status**: accepted
- **Context**: A multi-question `AskUserQuestionCard` rendered full-height in the drawer's stuck-question callout and inside every waiting multi-agent sub-run card, burying the drawer's content below it; and a rejected answer IPC (turn already ended) left sub-run cards up forever (`.then` with no catch).
- **Consequences**: `StuckQuestionCallout` and the sub-run cards show a one-line amber strip first (expand to answer, dismiss hides only that request_id; a new request resets both), and all three sub-run answer handlers now clear the card on failure as well as success.


## 2026-08-27 — Mass-retry requeues every terminal phase as one combined turn (token-runaway postmortem)
- **Status**: accepted
- **Context**: Postmortem of run `run-731fddd1` (2026-08-26, ~27.3M billed input tokens, ended on a 429 five-hour usage limit): the night run died wholesale before any LLM turn, and the only recovery path was per-phase `requeue_run_phase` — each retry produced a fresh single-phase session that re-read `loops.md`/PRD/context from scratch (5 sessions × ~70–110K context × 30–90 calls), re-ran the full verify→ship stack per loop, and the final manual dispatch sent a step explicitly marked "needs a human" to an agent. The combined-batch machinery (`next_queued_batch` grabs every simultaneously-`Queued` phase into one session with one verify→ship) already existed but was never engaged, because per-phase retry only ever requeues one phase at a time.
- **Consequences**: New `requeue_failed_run_phases` IPC + "Retry failed (N)" button in `RunQueuePanel` (shown when the run is inactive and ≥1 phase is parked/failed/interrupted/killed): requeues every terminal phase at once and restarts, so the retry executes as ONE combined session with a single verify→ship and draft PR — the cost of a wholesale-failed run's recovery no longer scales with phase count. Follow-ups still open: archive `loops.md` history (file is 150KB and read at every session start), enforce token budgets against actual usage, treat 429 usage-limit as terminal-park not retry, and gate plan-less dispatch paths (multi-agent "next unchecked step") on human-only step markers.

## 2026-08-28 — Multi-agent run cards bounded and collapsed; the conversation keeps the panel
- **Status**: accepted
- **Context**: Finished multi-agent runs stacked as full-height cards in the Agent panel (the section was `shrink-0` and unbounded), squeezing the conversation transcript into an unreadable strip.
- **Consequences**: The `MultiAgentRuns` section is capped at 45% of the panel column with an internal scroll area, and terminal runs (done/failed/cancelled) collapse to one compact row (agent names + status + relative time + one-line result/error, click to expand); non-terminal runs stay expanded.

## 2026-08-28 — Streaming-fresh pipeline converts `is_error` responses to `Err` (429 no longer shows as completed)
- **Status**: accepted
- **Context**: `start_fresh_and_record_streaming_in_root_with_config` returned `Ok` for CLI-level errors (e.g. 429 rate limits) even though the non-streaming `send_and_record` converts them, so multi-agent manifests and the run queue persisted failed runs as done.
- **Consequences**: The streaming-fresh funnel now mirrors `send_and_record` — records the assistant turn, then returns `Err(AppError::Agent(…))` on `is_error` — so multi-agent sub-runs persist as `failed` and run-queue phases stop reporting fake completions; `.loopdeck/agent-runs/` added to `.gitignore`.
## 2026-08-26 — Night-variant gauges hardcode limits.rs defaults in TS; run-plan presentation single-sourced in lib/nightRun.ts
- **Status**: accepted
- **Context**: `RunBudgets` caps are `Option<u64>` where None means "backend applies `limits::DEFAULT_RUN_*` at execute time," no IPC exposes those constants, and adding one is a `prd-night-run-surfaces` Non-Goal — run-queue clarification picked hardcoded TS mirrors purely for display math.
- **Consequences**: `src/lib/nightRun.ts` carries `DEFAULT_RUN_PHASE_TOKEN_CAP/_WALL_CLOCK_SECS/_TOTAL_WALL_CLOCK_SECS` literals (sync-commented to `limits.rs:102-108`) so the 3 gauges always show a real fill; `RunQueuePanel.tsx`'s `STATUS_LABEL`/`STATUS_COLOR`/`parseParkedQuestions` relocated there as the single source shared with `NightRunTab.tsx`, fixing a pre-existing `slice(start + 14)` off-by-one (marker is 13 chars) that dropped the JSON's leading `[` and silently disabled structured `__QUESTIONS__` parked-question parsing in `RunQueuePanel`'s morning report.

## 2026-08-28 — Plan-tonight wizard: createRunPlan at the 1→2 transition; live-interview parks answer via the shared slot, not answerParkedQuestion
- **Status**: accepted
- **Context**: `prd-night-run-surfaces` Phase 2's wizard needed `createRunPlan` (which takes queue-time draft-PR consent) before its step 2 could run interviews, yet the consent UX sits at step 3 — a circular ordering. Separately, the run's pre-answered clarification said a parked interview question "answers pin via `answerParkedQuestion`", but that command requires `phase.status === "parked"` (mid-run park), while a *pre-flight* interview phase stays `queued` and parks its `AskUserQuestion` on the shared pending-question slot that chat's callouts already consume.
- **Consequences**: `createRunPlan` fires on the wizard's step 1→2 transition with the draft-PR consent checkbox pre-checked (matching `RunQueuePanel`'s default); step 3's required checkbox gates only the final `queueRun` action. Abandoning the wizard mid-way leaves the plan queued-but-unstarted — the same state the existing panel leaves, and re-entering replaces it (`createRunPlan`'s existing replace semantics, no new backend behavior). For parked interview questions, the wizard polls `listPendingQuestions` at 1s while a turn runs, renders the shared `AskUserQuestionCard` inline, and submits via `agentAnswerQuestion` — the slot's answer path — which unblocks the turn so `runPhaseInterview` resolves with the answers pinned. Documented deviation from the clarification's literal wording, honoring its intent (inline card, pinned answers) with the command that actually resolves the state it finds. The wizard's final action passes only the project path to `queueRun`, which re-reads `run-plan.yaml` — so the phase/budget/consent payload shape is exactly what `createRunPlan` (and through it `build_run_plan`) wrote, with no frontend-reconstructed payload to drift.

## 2026-08-29 — Chat support code is isolated by responsibility
- **Status**: accepted
- **Context**: `Chat.tsx` mixed chat orchestration with reusable formatting, transcript normalization, and both parked-approval controls, making a high-churn surface hard to scan safely.
- **Consequences**: Moved pure chat helpers into `chatUtils.ts` and approval UI into `ApprovalCards.tsx`; `Chat.tsx` preserves its existing public exports as re-exports and remains the composer/transcript coordinator.

## 2026-08-29 — Harness parking contract is separate from the Claude process
- **Status**: accepted
- **Context**: The Claude and Codex harnesses both depend on the pending-question, permission, plan, and interrupt-slot types, but they were defined inside the 2,800-line Claude session implementation.
- **Consequences**: Moved that shared contract to `claude_session/parking.rs` and re-exported it through `claude_session`, leaving downstream imports and runtime behavior unchanged.

## 2026-08-30 — Night-run status supports, rather than replaces, the agent conversation
- **Status**: accepted
- **Context**: The night-run Agent tab replaced the chat transcript with phases and budgets, and answering a structured parked question requeued it without restarting execution.
- **Consequences**: The Agent tab now keeps chat mounted with a compact, collapsible run-status companion; Answer starts the requeued run so agent activity resumes visibly.

## 2026-08-30 — Multi-agent detail is on demand
- **Status**: accepted
- **Context**: The launcher and historical sub-run cards occupied much of the Agent tab even when the user only needed the conversation.
- **Consequences**: A compact agent-name row now opens each sub-run in a popover, and profile selection plus launch live in a separate Run popover.

## 2026-08-30 — Draft-PR delivery is terminal even when its PRD ID drifts
- **Status**: accepted
- **Context**: An agent could verify PASS and create a draft PR after changing/removing the queued stable ID, causing the completed work to look unmatched and tempting a costly rerun.
- **Consequences**: Verification auto-relinks only an exact epic/PRD/phase/title match; otherwise the phase remains non-retryable `delivered` with PR evidence and a guided fallback relink.
## 2026-08-30 — Morning report renders in the Agent-tab slot with a stay-on-report latch; parked inbox single-sourced
- **Status**: accepted
- **Context**: `prd-night-run-surfaces` Phase 3. The PRD predates Phase 1's outcome — the "night variant of the drawer" turned out to be a swap inside the Agent tab (`NightRunTab` replaces `AgentPanel` while a run is active), not a whole-drawer variant, so the morning report needed a mount decision. Its "collapsible audit-log tail" also assumed raw log lines, but `AuditSlice` only carries `auto_allow_count` + `floor_denials` and the PRD's Non-Goals forbid new endpoints. Three more pre-answered clarifications: rail-door-only indicator (no `RoomCard` exists yet), badge clears once the report is opened, and "Answer & requeue" must run both night-variant requeue paths while the report stays mounted and refetches.
- **Consequences**: `MorningReportTab.tsx` mounts in the same Agent-tab slot (`ProjectDrawer` render priority: latched report → night variant → `AgentPanel`). Readiness is `morningReportReady` (`lib/rail.ts`): plan exists, nothing active/queued, ≥1 terminal phase — a halt-on-stall plan with queued phases left stays on the night variant's parked inbox, so the two surfaces never fight over one state. A latch in `ProjectDrawer` keeps the report mounted after a requeue reactivates the run (same plan id) until the drawer closes or a new plan appears; the tab's 5s `getRunReport` poll refetches so requeued phases show fresh verdicts. NightRunTab's parked card + both requeue handlers were extracted verbatim into shared `ParkedQuestionInbox.tsx`, which is why the report's requeue wiring cannot drift from the night variant's. The audit tail is a native `<details>` rendering only what `AuditSlice` carries. The rail sun badge is gated by transient `appStore.morningReportSeen` (path → plan id): clear-once-opened, re-armed by a new plan; transient by the store's documented policy, so it legitimately returns after an app restart.

## 2026-08-31 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on Selasar development.

## 2026-08-31 — PR-backed runs complete the PRD checklist idempotently
- **Status**: accepted
- **Context**: A successful unattended PR completed runtime state without marking its spec checklist item, leaving delivery state to drift.
- **Consequences**: New runs live under `.loopdeck/runs/<branch-name>/`; after recording a draft PR, the runner checks the matching PRD item without ever reopening one on retry.

## 2026-08-31 — Pre-flight interviews can be answered as one contextual batch
- **Status**: accepted
- **Context**: A multi-phase night plan required a separate tap and agent turn for every pending interview, fragmenting the shared work context.
- **Consequences**: The wizard now offers one combined interview that presents all questions together and pins the phase-tagged answers back to their respective loops.

## 2026-08-31 — Delivery links live on the execution.yaml loop records; reconciliation is a pure evaluator
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 1. Branch/commit/PR/rubric links had nowhere persisted to live, and "conflicting records" was defined nowhere — the UI report, the delivery gates, and tests each risked their own drift definition.
- **Consequences**: New `src-tauri/src/delivery.rs`: `DeliveryLinks` (branch, commit, pr_url, optional pr_provider, optional `RubricResult`) persisted as an optional field on both `ActiveLoop` and `HistoryLoop` in `execution.yaml` (copied through `complete_current`/`abandon_current`), plus a pure `reconcile_delivery(links, LiveDeliveryState) -> Vec<MismatchKind>` and `evaluate_delivery_gates(...) -> Vec<GateBlock>` shared by report, gates, and tests. `extract_rubric_result` parses a `loopdeck-prd-verifier` report (per-criterion rows + `**Verdict:**`, last occurrence wins) into the retained `RubricResult`; provider stays optional because non-GitHub support is an open PRD question.

## 2026-08-31 — Run worktrees consolidate under `.loopdeck/runs/` and resume recreates from the surviving branch
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 2. Run worktrees scattered across legacy locations (`.loopdeck-runs/`, `.loopdeck-agent-worktrees/`) with no single managed home; a deleted worktree directory killed an otherwise-healthy run even when its branch survived.
- **Consequences**: `run_queue.rs::ensure_worktree` and `multi_agent.rs` both place worktrees under `.loopdeck/runs/` (gitignored via idempotent `ignore_managed_runs_dir`). On resume, a missing worktree directory is recreated at the same path from the surviving branch (`git worktree add` with the existing branch checked out); only a missing branch (or no recorded branch) errors. External/legacy worktrees are detected and classified (`detect_external_worktrees`: managed / legacy-run / legacy-multi-agent / claude-harness / user-manual) for the delivery report — never moved or deleted.

## 2026-08-31 — Delivery gates run a fresh rubric and own the loop's terminal state
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 3. Checklist completion could happen without branch/PRD/rubric/PR evidence, and a rerun after a delivered PR risked double-delivering.
- **Consequences**: `execute_run`'s success path parses the turn's own verifier report into a `RubricResult` and evaluates `evaluate_delivery_gates` fresh (loop pending, branch match, PRD link, rubric all-pass); any block parks the batch before any completion mutation. `DeliveryLinks` (branch, head commit, PR URL, provider, rubric) are persisted onto the loop before `complete_current`, and the PRD checklist items are checked only after the draft PR exists (`epic::complete_prd_loop`, idempotent). An idempotent-finish path (all checklist items already checked + an open PR on the branch) bypasses the gates so a retry after a successful delivery completes instead of re-delivering.

## 2026-09-01 — Session heartbeat
- **Status**: proposed
- **Context**: AI session active on Selasar development.

## 2026-09-01 — Clean handoff is lazy: record at delivery, cut the next worktree from the default branch at next run start
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 4, loop `clean-handoff`. PRD Open Question #1 (eager vs lazy next-branch creation) and the base of the next branch were pre-answered for the unattended run: lazy, base on the default branch, keep the delivered worktree.
- **Consequences**: New `src-tauri/src/handoff.rs` (`HandoffRecord` → `.loopdeck/handoff.yaml`: delivered branch, PR URL, retained worktree, `next_base`), persisted best-effort by `run_queue.rs::record_handoff` right after PR creation — it never fails the delivery. `ensure_worktree` now cuts every new run branch from `git::default_branch` (main/master, fallback HEAD) instead of whatever the main worktree has checked out, so a stray or delivered checkout can't leak into the next run's base; `finalize_worktree` retains the delivered worktree for review (supersedes prune-on-full-success). The `.loopdeck/runs/<next-branch>/` worktree itself is created only when the next run starts — no idle trees.

## 2026-09-01 — Failed deliveries persist a stage record; one idempotent retry resumes, nothing auto-retries
- **Status**: accepted
- **Context**: `prd-verified-delivery-reconciliation` Phase 4, loop `retry-recovery`. Pre-answered clarification: no automatic retries; record how far the delivery got and expose one idempotent retry command (PRD P0 "offers the idempotent next action").
- **Consequences**: New `src-tauri/src/delivery_retry.rs`: `DeliveryStage` (nothing_mutated / committed / pushed) detected live from Git (`detect_stage`: dirty tree → nothing; clean + HEAD on a remote ref → pushed), `DeliveryRetryRecord` → `.loopdeck/delivery-retry.yaml` written by the executor at the failure site (with the retained rubric), and `run_retry` — re-detects the live stage, then requeue (nothing mutated) / push → adopt-or-create draft PR → finish bookkeeping (checklist → plan phases → `record_recovered_delivery` flips Abandoned → Completed with links) → clears the record. `gh` runs via PATH lookup so tests stub it with a script dir; push/PR failure keeps the record recoverable at the live stage. UI: `retry_delivery` command + `DeliveryReportTab` RetryCard (reason + `next_action`) and HandoffBanner.

## 2026-09-01 — Role charter lives flattened on NamedAgentConfig, advisory-only
- **Status**: accepted
- **Context**: `prd-role-foundations` Phase 1, loops `charter-model` + `charter-crud`. Pre-answered: charter on the global roster only (no `.loopdeck/` override layer); advisory prose, no post-run validation; `allowed_skills` suggests, never restricts.
- **Consequences**: New `RoleCharter` (`config.rs`: `persona_prompt`, `allowed_skills`, `output_contract`, all optional) `#[serde(flatten)]`ed into `NamedAgentConfig` beside the flattened `AgentConfig` — YAML/IPC shape stays flat, old `config.yaml` loads unchanged (empty charter = plain connection profile, ADR-3). `update_agent_config` (connection edits) preserves the charter; new `update_agent_charter` replaces it wholesale (normalized: trimmed prose, empty → cleared from YAML). New IPC command `update_agent_charter` (config_cmds, in-memory rollback, no secrets). Editor: "Role charter (optional)" section in `AgentConfigEditor` (persona/output-contract textareas, comma-separated skills input); `AgentRoster.save` issues the charter call after create/update. No enforcement anywhere — Phase 2 injects into prompts, Phase 3 makes policy enforceable.
