---
prd: prd-attention-view
epic: execution-foundations
milestone: "unassigned"
status: proposed
description: >
  Show the active run, next required decision, and verified delivery result
  together in each project's primary view using the authoritative run model.
order: 50
---

# PRD — Attention View

## Overview

Expose the new read model as a clear answer to three questions: what is
running, what needs me, and what was delivered. Keep detailed chat, epics,
queue planning, and reports reachable from that summary.

## Problem Statement

The current product presents agent chat, run queue, morning report, and
delivery evidence in different surfaces. A user can need to compare them to
understand the latest state. The new domain model should remove that work.

## Goals

| Priority | Goal |
|---|---|
| P0 | Produce one project summary from the authoritative run snapshot and replayed events. |
| P0 | Surface the oldest actionable pending decision, including run and phase context, and route the answer to the correct decision ID. |
| P0 | Show terminal outcome and verified delivery evidence without inferring success from prose or a green-looking badge. |
| P1 | Preserve access to detailed chat, spec, queue, and report views. |

## Non-Goals

- A complete visual rebrand or replacement of the existing navigation.
- New automation, provider capability, or delivery policy.
- Auto-answering pending decisions.

## Design

Create a backend project-summary projection keyed by stable IDs and revision.
The UI should display one active run, a concise status timeline, the next
required decision, and delivery evidence. When multiple runs or decisions
exist, show counts and a route to details; do not silently select by project
path or arrival order. Empty, offline, interrupted, parked, failed, and
delivered states need distinct copy and actions.

## Phases

### Phase 1 — Summary read model

- [ ] `attention-view/project-summary` Add a backend project-summary query with active runs, pending decisions, terminal outcomes, and verified delivery references; define tie-breaking and stale-data behavior.
- [ ] `attention-view/decision-routing` Route decision answers by stable decision ID and revision, rejecting an answer to a resolved or changed request.

### Phase 2 — Primary view

- [ ] `attention-view/render-summary` Show run status, next required action, and verified delivery in the project's primary view with links to detailed chat, plan, and report.
- [ ] `attention-view/cover-states` Add clear empty, offline, interrupted, parked, failed, and delivered presentations without hiding conflicting evidence.

### Phase 3 — Verification

- [ ] `attention-view/test-journey` Test start, navigate away, restart, answer a parked decision, and inspect delivery; assert the summary agrees with detailed views at each step.
- [ ] `attention-view/accessibility-check` Verify keyboard access and screen-reader labels for pending decisions and status changes; run a manual desktop smoke on both providers.

## Acceptance Criteria

- From one project view, a user can identify the active run, next required
  decision, and verified delivery evidence when present.
- Answering a decision cannot target a different run after navigation or
  replay; stale answers receive a clear conflict message.
- The summary and detail views agree after restart for queued, parked,
  interrupted, failed, and delivered fixture states.
- The existing detailed views remain accessible throughout rollout.

## Open Questions

- Should a project with multiple simultaneous runs show a primary run or a
  ranked list? Resolve with actual concurrent-run cases before UI work.
