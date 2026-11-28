---
artifact: arbitration-report-records
author_role: planner
phase: arbitration/report-records
type: report
created: 2026-11-28
summary: Both morning report views show the decision maker, options, rationale, review history, and recorded escalation for each adjudicated phase.
---

## Summary

The morning report reads adjudication evidence from the saved run plan. Each record belongs to one phase.

## Requirements

- R1: Identify the assigned decision maker and show the recorded options in their original order.
- R2: Show the agent's rationale and distinguish the agent's selection from a human selection.
- R3: List each review with its artifact, outcome, and rationale in the recorded order.
- R4: Show the phase's recorded escalation reason and any human resolution, even when the phase has no verdict.
- R5: Omit the adjudication section when the run contains no adjudication records.

## Constraints

- C1: The producer stores one escalation per phase. The report can show only the escalation retained in the snapshot.
