---
prd: prd-provider-contract
epic: execution-foundations
milestone: "unassigned"
status: proposed
description: >
  Normalize Claude and Codex session output into one versioned run-event
  contract, with provider fixtures and one generated Rust-to-TypeScript shape.
order: 20
---

# PRD — Provider Contract

## Overview

The existing `HarnessAdapter` is the boundary. Make its outward event model
provider-neutral before persisting events or rebuilding UI projection. Keep
provider-specific wire payloads available in diagnostic logs.

## Problem Statement

Shared UI and IPC types still carry provider-specific naming and adapter
details. A replayable history needs stable event meaning across Claude and
Codex, especially for approvals, tool activity, usage, and termination.

## Goals

| Priority | Goal |
|---|---|
| P0 | Define a versioned `RunEvent` envelope with run ID, sequence placeholder, timestamp, event ID, and provider-neutral payload variants. |
| P0 | Normalize both providers into equivalent start, content, tool, decision, usage, and terminal semantics. |
| P0 | Preserve provider raw IDs/payload references for diagnosis without exposing them as product identity. |
| P1 | Generate or validate TypeScript bindings from the Rust contract and remove duplicated hand-maintained shape for migrated events. |

## Non-Goals

- Change how either provider is launched or billed.
- Delete legacy `ClaudeEvent` consumers before replay migration.
- Promise identical capabilities where the providers differ; unsupported
  capabilities must be explicit.

## Design

Introduce conversion functions at the provider boundary and keep a temporary
adapter to the current channel payload. The normalized envelope must specify
what is durable versus transient, how redaction works, and how unsupported
provider features are represented. A contract test should load captured,
sanitized fixtures from both providers and compare the resulting semantic
events, not their wire syntax.

## Phases

### Phase 1 — Event vocabulary

- [x] `provider-contract/define-events` Define the versioned event envelope and payload table, including capability differences, redaction, and terminal outcome rules. See [provider-event-contract.md](provider-event-contract.md) and `src-tauri/src/provider_event.rs`.
- [ ] `provider-contract/capture-fixtures` Add sanitized Claude and Codex fixtures for text, tool use, approval/question, usage, error, interrupt, and successful completion.

### Phase 2 — Provider normalization

- [ ] `provider-contract/normalize-claude` Convert Claude output at its adapter boundary while preserving the current channel behavior.
- [ ] `provider-contract/normalize-codex` Convert Codex output to the same semantic events while preserving the current channel behavior.
- [ ] `provider-contract/share-types` Generate or mechanically validate TypeScript event bindings from Rust and migrate one consumer to them.

### Phase 3 — Verification

- [ ] `provider-contract/test-contract` Run fixture-based contract tests for both providers and a frontend type/render test for the migrated consumer; verify no duplicate terminal event or lost approval payload.

## Acceptance Criteria

- The same product-level event variants describe equivalent behavior from
  both providers, while differences remain explicit in capability metadata.
- Fixtures cover both successful and interrupted/error sessions for each
  provider and prove exactly one terminal event per run segment.
- A CI check detects Rust/TypeScript event contract drift.

## Open Questions

- Which raw provider fields are safe to retain for diagnosis? Resolve with
  the existing secret/redaction rules before fixtures are committed.
