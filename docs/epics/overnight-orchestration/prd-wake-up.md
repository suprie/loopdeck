---
prd: prd-wake-up
epic: overnight-orchestration
milestone: "0.4.0"
status: proposed
description: >
  The morning half of the accountability story: OS notifications when the run
  finishes, dies, or fully parks, and a morning report view with per-phase
  verdicts, draft-PR links, a parked-question inbox that requeues phases on
  answer, and the overnight audit slice of auto-allowed and floor-denied
  decisions.
---

# PRD — Wake-Up

## Overview

Overnight autonomy is only defensible if the morning review is effortless and
complete. This PRD ships the two surfaces that close the loop: notifications
(the wake signal) and the morning report (the review surface). The report is
what makes `FullAccess`-overnight auditable — every auto-allowed decision and
every floor denial from the run window, in one place.

## Problem Statement

`Cargo.toml` carries only `tauri-plugin-dialog` and `tauri-plugin-shell` — no
notification capability exists, so a finished (or killed) run is silent. And
there is no aggregated view of "what happened last night": verdicts live in
`execution.yaml`, PRs on GitHub, parked questions in the run plan, audit
decisions in the log files. Morning-you would have to forensically reassemble
the night before trusting any draft PR.

## Goals

| Priority | Goal |
|----------|------|
| P0 | OS notification on: run completed, budget kill, all remaining phases parked |
| P0 | Morning report view: per-phase table with verify result, PR link, park/kill reason, token + wall-clock usage |
| P0 | Parked-question inbox: pending payloads rendered as the existing question cards; answering requeues the phase |
| P1 | Overnight audit slice: auto-allowed decisions and floor denials during the run window, surfaced in the report |
| P2 | Notification click focuses the report view |

## Non-Goals

- Push/mobile/email notifications — OS-local only; the user's phone is out of
  scope for 0.4.0.
- Historical analytics across runs (trends, cost dashboards) — the report
  covers *the last run*; history stays in `execution.yaml` and git.
- Re-verifying or re-scoring the night's work — the report *presents* the
  verify verdicts and evidence; it does not re-run the verifier.

## Design

Directional; refine during implementation.

- **Notifications**: `tauri-plugin-notification` (official v2 plugin) +
  capability entry in `capabilities/default.json`. Emission points sit in the
  executor's terminal transitions (run completed / killed / fully parked) —
  three call sites, no notification framework.
- **Report data**: a single IPC command (`get_run_report`) that joins what
  already exists — the `RunPlan` (statuses, park payloads, budgets used),
  `execution.yaml` records (verify verdicts, delivery evidence via the 0.2.1
  read model in `progress.rs`), and the audit log filtered to the run window.
  No new storage; the report is a read model.
- **Inbox**: parked payloads are the same `AskUserQuestion`/permission shapes
  the chat already renders — reuse those cards. Answering writes the pinned
  answer into the plan and flips the phase back to `queued`; the user chooses
  whether to start the requeued run attended or unattended.

## Phases

### Phase 1 — Notifications

- [ ] Add `tauri-plugin-notification` with its capability entry
- [ ] Notify on run completed, budget kill, and all-remaining-phases-parked; clicking focuses the report view

### Phase 2 — Morning report

- [ ] Report view: per-phase verdict table (verify result, PR link, park/kill reason, token + wall-clock usage) via a `get_run_report` read-model command
- [ ] Parked-question inbox: render pending payloads as the existing question cards; answering pins the answer and requeues the phase
- [ ] Overnight audit slice: auto-allowed decisions and floor denials during the run window, from the existing audit path

### Phase 3 — Tests

- [ ] `npx tsc --noEmit` green; report rendering against a fixture run plan covering completed, parked, and killed rows

## Open Questions

- Where does the report live in the UI — a tab on the run-queue view, or a
  standalone route the notification deep-links to? Bias: same surface as the
  run-queue view (one place for "the run", live and post-hoc).
- Audit-slice granularity: every auto-allowed call (potentially hundreds) or
  a summarized count with floor denials itemized? Bias: summarize allows,
  itemize denials — denials are the signal.
- Does answering a parked question auto-resume the run unattended, or always
  require an explicit re-start? Bias: explicit re-start — morning answers
  deserve a conscious "go again".
