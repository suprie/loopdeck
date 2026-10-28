# Domain Contract — State Inventory

**Status:** Phase 1 evidence, 2026-10-14  
**Scope:** the current LoopDeck project checkout and the Rust/Tauri runtime

This inventory names the current owner of every mutable value that can affect a
run, phase, session, decision, or delivery result. A path is a locator only;
the current implementation still uses it as the key for several in-memory
maps. That is the primary ownership conflict to remove in later phases.

## Identity inventory

| Contract identity | Current representation | Current writer | Current authoritative reader | Gap |
|---|---|---|---|---|
| `ProjectId` | None; project is keyed by canonical `PathBuf` | `project.rs` and project registry updates | `commands/state.rs::resolve_root`, project registry | Add a stable UUID/name independent of a moved path. |
| `RunId` | `RunPlan.id` (`run-<UUID>`) for queued runs; UUID for multi-agent manifests; absent for ordinary interactive conversations | `commands/run_queue.rs:532`, `multi_agent.rs:357` | `runplan::load`, `multi_agent::load_manifest` | Interactive turns need an explicit run identity; legacy mapping must be deterministic. |
| `PhaseId` | `RunPhase.execution_id` and `PrdLoop.id` | PRD/spec layer and `create_run_plan` | `run-plan.yaml`, `execution.yaml` | This is already the stable external key; preserve it unchanged. |
| `SessionId` | Provider session string in `ConversationTurn.session_id`; Codex prefixes thread IDs with `codex:` | Claude/Codex adapters; transcript append path | `conversation::last_session_id`, provider resume call | Provider session IDs are not LoopDeck run/session IDs and can be absent on errors. |
| `DecisionId` | Provider `request_id` for pending question, permission, or plan approval | Claude control-request parser and pending-slot writers | `agent_answer_*` commands validate the current slot's request ID | No durable decision record; the ID is only live-memory scoped. |

## Durable mutable state

| State / fields | File and owner today | Writers | Readers | Migration authority |
|---|---|---|---|---|
| Current loop, queue, history, revision, attempt, outcome, delivery links | `.loopdeck/execution.yaml`; `execution.rs` is the validated writer | `commands/execution.rs` transitions; `commands/run_queue.rs` completion/abandonment paths; CLI state path | execution IPC, run queue reconciliation, derived UI | Keep file authoritative through the pilot. Revision/OCC remains the conflict check. |
| Run plan ID, project path, consent, budgets, environment/worktree, phase statuses, interview answers, dependencies, usage, park/failure payloads | `.loopdeck/run-plan.yaml`; `runplan.rs` serializer | `commands/run_queue.rs` creates and updates plans; `run_executor.rs` mutates phase state | `queue_run`, `get_run_status`, report builders, frontend polling | Keep as compatibility export while a new store is piloted. |
| Multi-agent run/sub-run status, assignment snapshot, branch/worktree, result/error timestamps | `.loopdeck/agent-runs/<run UUID>/manifest.json`; `multi_agent.rs` | `save_manifest` plus `update_subrun` under per-project async lock | list/control commands; `reconcile_stale_runs` | Manifest remains readable until imported and compared. |
| Conversation turns, tool calls, task records, usage, terminal/error marker, provider session ID | `.loopdeck/sessions/active.jsonl` and `archive-*.jsonl`; `conversation.rs` | `conversation::append_turn`, `archive_conversation`; called by `commands/agent.rs` and queued/multi-agent start paths | conversation list/load, `last_session_id`, frontend transcript | Transcript is evidence/history, not the authoritative run status. |
| Multi-agent provider stream events | `.loopdeck/agent-runs/<run UUID>/<agent UUID>.jsonl` | `multi_agent::append_transcript` | diagnostic/history paths | Import as event evidence; do not infer lifecycle status from raw events. |
| Delivery links/rubric, handoff, failed-delivery stage | `.loopdeck/execution.yaml`, `handoff.yaml`, `delivery-retry.yaml` | delivery pipeline, retry pipeline, handoff recorder | delivery report, retry command, handoff consumers | Delivery gate remains authoritative until the new store has equivalent evidence. |

## In-memory state

`AppState` (`src-tauri/src/commands/state.rs:37-82`) owns the following
ephemeral values, all keyed by project path: cached provider sessions,
pending question/permission/plan slots, interrupt slots, active run handles,
multi-agent admission locks, and manifest locks. The pending slots contain the
current `request_id` and payload, but no durable record is written. A restart
therefore correctly drops the live process, but currently cannot reconstruct a
pending decision or distinguish an interrupted ordinary interactive turn from
an idle one without reading transcript heuristics.

The frontend adds another non-authoritative layer: streaming channels and
Zustand state in `src/store/streamingState.ts` and `src/store/appStore.ts`.
These are presentation caches; they must be rebuilt from a backend snapshot or
replay cursor in later PRDs.

## Command ownership matrix

| Command family | Mutates | Reads | Required future owner |
|---|---|---|---|
| `agent_start_loop`, `agent_send_message`, streaming variants | session map, transcript, provider process | pending slots, last session ID | run transition service + session adapter |
| `agent_answer_question`, `agent_answer_permission`, `agent_answer_plan` | pending slot and provider control response | current request ID | decision service keyed by `DecisionId` |
| `queue_run`, `cancel_run`, `get_run_status`, interview/requeue/kill | run plan, execution state, run handle | plan and execution state | run transition service |
| multi-agent start/control/list | manifest, worker admission set, sub-run transcript | manifest, live slots | run/sub-run service |
| delivery/retry/report | delivery links, retry/handoff records, execution/plan terminal fields | branch/PR/rubric state | delivery evidence owner; it must publish an idempotent result |

## Authority decisions for migration stages

1. **Before store import:** existing files and provider transcripts remain
   authoritative. The new store is absent.
2. **Pilot:** the new store owns imported mutable snapshots only after a
   successful transaction; `execution.yaml` and `run-plan.yaml` remain the
   compatibility export. Any projection mismatch is a visible diagnostic and
   blocks cutover.
3. **Default read path:** the new store is authoritative for mutable run,
   phase, session, decision, and delivery state. Repository-authored specs,
   decisions, and stable phase IDs remain file-authoritative.
4. **Rollback:** restore the pre-import backup, disable the new read path, and
   continue from the compatibility files. No source file is deleted by the
   migration.

## Conflicts found

- `run-plan.yaml` and `execution.yaml` both carry phase/loop progress and are
  updated by different command paths; neither currently owns the complete run.
- `AppState.run_handles` says whether a queued executor is live, while the
  persisted plan says what happened. This split is intentional today but must
  become a derived process observation after import.
- Ordinary interactive sessions have a provider session ID but no LoopDeck
  run ID; multi-agent and queued runs have the inverse problem of multiple
  provider sessions under one logical run.
- Pending decisions are protected against stale answers with `request_id`, but
  they are not durable and therefore cannot survive app restart.

