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
