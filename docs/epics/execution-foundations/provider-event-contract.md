# Provider Contract — Versioned Run Events

**Status:** Phase 2 normalization, 2026-10-28
**Version:** `1`

The provider boundary converts Claude and Codex wire messages into one semantic
envelope. The current `ClaudeEvent` channel remains a compatibility surface
until normalization is migrated. During Phase 2 both surfaces are emitted from
the same adapter code path (dual-emit): legacy payloads are unchanged, and the
normalized `RunEvent` stream is additive.

## Envelope

```text
RunEvent {
  version: 1,
  run_id: stable LoopDeck run identity,
  sequence: null until the event store assigns a per-run number,
  event_id: adapter-generated idempotency key,
  timestamp: UTC timestamp,
  provider: claude | codex,
  payload: provider-neutral variant,
  raw_ref: optional diagnostic-only provider id + payload digest
}
```

`run_id` is product identity. Provider session IDs and request IDs are payload
data or diagnostic references and must never replace it.

## Payload table

| Variant | Durable | Required meaning |
|---|---|---|
| `started` | Yes | Run segment began; includes provider capability snapshot and optional provider session ID. |
| `content` | Yes | Ordered text or thinking content; `thinking` is subject to the existing diagnostic/redaction policy. |
| `tool` | Yes | Tool name plus redacted input; raw input is not part of product state. |
| `decision` | Yes | Stable decision ID, approval/question kind, pending/resolved state, optional redacted summary, and optional `detail_redacted` render payload (question specs JSON, plan markdown, or `{tool_name, input}`). |
| `usage` | Yes | Normalized input/output token counts and optional cost; adapter handles cumulative-vs-delta differences. |
| `retrying` | No | Transient UI signal between provider attempts; the final terminal event remains authoritative. |
| `terminal` | Yes | Exactly one terminal outcome for a run segment: completed, error, interrupted, cancelled, or killed. |
| `unsupported` | Yes | Explicit capability gap with redacted detail; never silently drops provider behavior. |

## Capability differences

Both providers expose text, thinking, tool activity, questions, approvals,
usage, and interrupt semantics through the contract. Claude currently exposes
`plan_approval`; Codex does not, so Codex capability metadata omits it and a
provider-specific plan request must become `unsupported` rather than being
relabelled as a normal approval.

## Redaction and raw diagnostics

- Durable product events contain redacted tool inputs, error messages,
  decision summaries, and decision render details. Normalization applies the
  same pattern set as the pre-push scan (`secret_scan::redact_secrets`) and
  truncates detail to 4 KB before the value becomes product state.
- `raw_ref` may retain a provider event ID and SHA-256 payload digest for
  diagnosis; it does not retain raw secrets or expose provider IDs as product
  identity.
- Raw provider payloads remain in provider-scoped diagnostics only after the
  existing secret-scan/redaction policy is applied. Fixture capture must use
  sanitized payloads.
- Attachments, auth tokens, environment variables, and full command output are
  never copied into the normalized envelope by default.

## Automatic approval

Selasar's autonomous policy (and a role rule that explicitly covers a request)
can approve an action without parking the human. The contract has no separate
`auto-allow` enum value; the action is `DecisionState::Allowed`, and automatic
provenance is encoded in the redacted summary with the stable prefix
`auto-allow: ` (see `provider_normalize::AUTO_ALLOW_PREFIX`). The morning
report and any future audit consumer match that prefix, so the distinction
survives normalization without widening the enum.

## Terminal rules

1. A run segment emits one and only one durable `terminal` event.
2. A retry may emit transient `retrying` events and failed-attempt diagnostics,
   but not a second durable terminal for the same segment.
3. `interrupted` means the process disappeared before a provider terminal
   result; resumption creates a new attempt/segment.
4. `error` means a provider or adapter failure was observed and recorded.
5. `cancelled` and `killed` are LoopDeck-owned outcomes and must not be
   inferred from provider text.

## Normalization (Phase 2)

Conversion lives in `src-tauri/src/provider_normalize.rs`:

| Wire source | Normalized mapping |
|---|---|
| Claude `system` init | `started` (session ID when present; duplicate starts suppressed) |
| Claude `assistant` text/thinking | `content` (`text` / `thinking`) |
| Claude `assistant` `tool_use` | `tool` with redacted input (`AskUserQuestion` becomes a decision instead) |
| Claude `control_request` | pending `decision` (`approval`, `question`, or `plan_approval`) |
| Claude `result` | one `usage` + one `terminal`; the user-interrupt marker maps to `interrupted` |
| Codex `turn/started` | `started` (thread ID) |
| Codex `item/agentMessage/delta` and reasoning deltas | `content` |
| Codex `item/started` tool items | `tool`; the completed agent message is suppressed when deltas already carried the text |
| Codex `thread/tokenUsage/updated` | `usage` from the turn-level `last` counters |
| Codex server requests | pending `decision`; answer/approval resolution emits the resolved decision |
| Codex `turn/completed` | exactly one `terminal` (`completed` / `error` / `interrupted` / `cancelled` / `killed`) |
| Codex dynamic tool call / unknown request | `unsupported` |
| Codex plan request (not a real capability) | `unsupported: plan_approval`, never a relabelled approval |

A retry attempt is a new run segment: the command layer starts a fresh
normalizer per `send_message_streaming` call, so each attempt can carry its own
terminal while the one-terminal-per-segment rule still holds. Event IDs are
`"<run_id>:<segment>:<n>"`.

The detached run queue emits normalized events on the app-wide `run-event`
topic (`RunEventEnvelope { project_path, execution_id, event }`) in addition to
the legacy `run-queue-event` payload. The migrated frontend consumer projects
that stream through the pure reducer in `src/lib/runEventProjection.ts`.

## TypeScript bindings

`src-tauri/src/provider_event.rs` is the single source of truth. `ts-rs`
derives generate `src/types/generated/*.ts`; run:

```bash
cd src-tauri && cargo test export_bindings
```

CI regenerates the bindings and runs `git diff --exit-code -- src/types/generated`,
so a Rust/TypeScript contract drift fails the build. Do not hand-edit the
generated files. Token counts and the optional sequence are generated as
`number` (well within JS safe-integer range) rather than `bigint`.

