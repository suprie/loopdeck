# Domain Contract — Phase 2 Transition Table

**Status:** Phase 2 contract, 2026-10-14

The transition layer is pure and provider/store agnostic. A retry never reopens
a terminal attempt; it creates a new attempt with a new idempotency key.

## Run and phase states

| From | Allowed destinations | Meaning |
|---|---|---|
| `queued` | `running`, `cancelled` | Admission has not started, or was cancelled before execution. |
| `running` | `parked`, `cancelled`, `killed`, `interrupted`, `delivered`, `completed`, `failed` | A live turn can await a decision, stop, exceed a hard kill budget, be recovered after restart, produce delivery evidence, finish, or fail. |
| `parked` | `queued`, `cancelled`, `killed` | A resolved decision may requeue the phase; cancellation/kill are terminal. |
| `delivered` | `completed` | Verification/delivery evidence exists; completion records the terminal outcome. |
| `cancelled` | none | Terminal user cancellation. |
| `killed` | none | Terminal budget/process kill. |
| `interrupted` | none | Terminal record for the lost process; resume is a new attempt. |
| `failed` | none | Terminal execution failure. |
| `completed` | none | Terminal successful outcome. |

`RunPhaseStatus` remains the serialized compatibility enum in
`run-plan.yaml`. The new pure layer maps it to the contract without changing
the YAML format. The first routed path is restart reconciliation:
`running → interrupted` in `run_executor::reconcile_running_phases`.

## Decision and approval states

`pending → answered | rejected | cancelled | expired` is the only allowed
decision family. The provider `request_id` is the legacy wire identity; the
future `DecisionId` must be durable and idempotent. A second answer against a
resolved decision is rejected, even if the provider accepts the same payload.

## Delivery evidence states

`pending → verified | failed` is the allowed delivery family. Retry does not
move `failed` back to `pending`; it creates a new delivery attempt and keeps
the failed record for auditability. `verified → completed` is represented by
the run/phase transition, not by mutating delivery evidence.

## Invariants

- Exactly one logical owner writes a mutable field at each migration stage.
- Terminal states are immutable; all retryable work has a distinct attempt.
- `delivered` requires evidence; `completed` without delivery is valid only for
  work that has no delivery gate.
- `interrupted` is the restart reconciliation outcome when the process is
  absent and no terminal provider result was durably recorded.
- A duplicate command must be rejected or be an idempotent no-op keyed by its
  command/idempotency key; it must never append a second terminal outcome.
- Persisted phase IDs remain the spec-layer `execution_id` values.

