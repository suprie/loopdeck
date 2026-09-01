---
prd: prd-handoff-spike
epic: role-based-orchestration
milestone: "0.6.0"
status: proposed
description: >
  Spike the epic's riskiest unknown before any orchestration scaffolding is
  built: validate that a file artifact written by one role-prompted session
  can be reliably consumed by a second session, using prompt text only and
  no new backend, and record findings plus a go/no-go into
  prd-agent-handoff's Design section.
---

## Overview

Two back-to-back interactive sessions in this repo: agent A (acting as a
business analyst via prompt text) produces a handoff artifact on disk;
agent B (acting as an engineering manager via prompt text) must consume it
and ground its plan in it. No roster changes, no scheduler changes — the
spike validates the *contract*, not infrastructure.

## Problem Statement

The entire epic's file-based blackboard design (ADR-2) is load-bearing on an
unverified assumption: that a downstream agent will actually read, respect,
and cite an upstream artifact rather than ignore it, truncate it, or drift
from it. If the assumption is wrong, prd-agent-handoff's design changes
fundamentally. This is cheapest to falsify now, with two manual sessions,
before role charters, handoff stores, or schedulers are built.

## Goals

| Priority | Goal |
|---|---|
| P0 | A written handoff artifact contract: schema, size caps, and a citation rule (what "consumed" measurably means) |
| P0 | A two-agent A-writes/B-consumes run on a real plan with observed behavior recorded |
| P0 | Findings (drift, truncation, ignored-input rate) and a go/no-go written into prd-agent-handoff's Design section |

## Non-Goals

- No backend, IPC, or roster changes of any kind
- No role charter model (that is prd-role-foundations)
- No automation of the handoff — the human runs both sessions

## Design

Stub — the spike's procedure lives in its loops. Candidate shape: agent A
writes `.loopdeck/handoffs/<topic>.md` per the drafted contract; agent B's
prompt names the artifact path and requires explicit citation of the parts
used; success is judged against the contract's citation rule.

## Phases

### Phase 1 — Spike: two-agent file handoff on a real plan

- [x] `handoff-spike/charter-contract` Draft the handoff artifact contract (schema, size caps, citation rule) as a reviewable doc
- [x] `handoff-spike/two-agent-run` Run A-writes/B-consumes back-to-back sessions on a real queued plan using prompt text only
- [x] `handoff-spike/findings` Record spike findings (drift, truncation, ignored input) and a go/no-go call into prd-agent-handoff's Design section

## Open Questions

- Artifact format: markdown with frontmatter, or JSON, or markdown wrapping a JSON contract block?
- Which repo and plan to spike against — a fresh throwaway plan, or real queued phases?
- What citation rate counts as "reliably consumed" for the go/no-go?
