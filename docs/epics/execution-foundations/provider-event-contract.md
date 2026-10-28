# Provider Contract — Versioned Run Events

**Status:** Phase 1 contract, 2026-10-14  
**Version:** `1`

The provider boundary converts Claude and Codex wire messages into one semantic
envelope. The current `ClaudeEvent` channel remains a compatibility surface
until normalization is migrated.

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
| `decision` | Yes | Stable decision ID, approval/question kind, pending/resolved state, and optional redacted summary. |
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

- Durable product events contain redacted tool inputs, error messages, and
  decision summaries.
- `raw_ref` may retain a provider event ID and SHA-256 payload digest for
  diagnosis; it does not retain raw secrets or expose provider IDs as product
  identity.
- Raw provider payloads remain in provider-scoped diagnostics only after the
  existing secret-scan/redaction policy is applied. Fixture capture must use
  sanitized payloads.
- Attachments, auth tokens, environment variables, and full command output are
  never copied into the normalized envelope by default.

## Terminal rules

1. A run segment emits one and only one durable `terminal` event.
2. A retry may emit transient `retrying` events and failed-attempt diagnostics,
   but not a second durable terminal for the same segment.
3. `interrupted` means the process disappeared before a provider terminal
   result; resumption creates a new attempt/segment.
4. `error` means a provider or adapter failure was observed and recorded.
5. `cancelled` and `killed` are LoopDeck-owned outcomes and must not be
   inferred from provider text.

The Rust contract is defined in `src-tauri/src/provider_event.rs`. TypeScript
bindings and adapter conversion functions are deliberately deferred to the
provider-normalization phase so the current channel behavior stays stable.

