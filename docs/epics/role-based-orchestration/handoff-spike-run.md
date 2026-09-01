---
title: Handoff Spike Run Record
epic: role-based-orchestration
prd: prd-handoff-spike
loop: handoff-spike/two-agent-run
status: completed
created: 2026-09-01
---

# Handoff Spike Run Record (A-writes / B-consumes)

Observed behavior for the `prd-handoff-spike` Phase 1 two-agent run, judged
against the [handoff artifact contract](handoff-artifact-contract.md).

## Setup

| | |
|---|---|
| Plan under test | Fresh throwaway synthetic brief (per Open Question #2 answer) — fictional "session transcript export" feature |
| Session A | Business-analyst role via prompt text only; wrote `.loopdeck/handoffs/session-export.md` (2,726 B) |
| Session B | Engineering-manager role via prompt text only; given the artifact **path only**, never its content; wrote `.loopdeck/handoffs/session-export-em-plan.md` (4,425 B) |
| Operator deviation | Per the pre-answered clarifying question, the loop-runner spawned both sessions back-to-back as subagents instead of a human running two interactive sessions (PRD Non-Goals wording deviation, authorized at queue time) |
| Sample size | 1 run, same harness for both sessions |

## Citation matrix (contract §5)

| Artifact part | Cited by B | Fidelity check |
|---|---|---|
| `#Summary` | yes | frames loops; no drift |
| `#Requirements` | yes | R1–R8 drive scope |
| `#R1` | yes | Loop 2 button, enablement matches |
| `#R2` | yes | chronological + speaker attribution matches |
| `#R3` | yes | one-line tool summaries, not raw JSON |
| `#R4` | yes | exact path + dir creation honored |
| `#R5` | yes | one-click, no dialog, derived filename |
| `#R6` | yes | header fields all four present |
| `#R7` | yes | temp+rename atomicity, failure modes scheduled |
| `#R8` | yes | abs-path toast, overwrite semantics |
| `#Constraints`, `#C1`, `#C2` | yes | offline + no-new-deps both honored |
| `#Non-Goals` | yes | used as scope fence, correctly |
| `#Open Questions`, `#Q1`, `#Q2` | yes | both resolved, citing normative items (R4, R3) |

**Coverage**: 17/17 headings + items cited; zero `not-used` needed.
**Fidelity**: no contradicting claims; no requirements fabricated or
attributed wrongly. Additions beyond the artifact (Windows rename caveat,
live-session snapshot) are grounded in repo reality and do not contradict it.
**Completeness**: no dropped, merged, or halved items — no truncation.

## Observed behavior notes

1. Both sessions read the contract doc from disk and complied with size caps
   (A: 2.4 KiB body; B: 4,084 B body, just under the 4 KiB soft cap).
2. B exceeded the contract minimum: inline `(R#)` markers throughout the plan
   body in addition to the required `## Handoff citations` block.
3. **Producer-side schema drift (the one negative finding)**: B's own
   artifact — itself `type: plan` — did not follow contract §3's body schema:
   it used `## Loops`, `## Risks`, `## Open question resolutions` instead of
   the schema's `## Requirements` / `## Non-Goals` / `## Open Questions`
   headings. Citation behavior (consumer role) was exact; heading-schema
   behavior (producer role) drifted. Contract implication: the §5 citation
   rule held without enforcement, while §3 needs either explicit
   allowed-heading extension for consumer-response artifacts or a producer
   check.
4. B resolved both open questions instead of leaving them for the human —
   reasonable here (both had normative anchors), but worth watching as a
   drift vector when artifacts lack anchors.

## Go/No-Go (bar: all key claims cited, no contradicting fabrications, no truncation)

**GO** — single-run sample: coverage 17/17, fidelity clean, completeness
clean. Failure modes to keep instrumenting once automated: producer-side
heading drift (observed), unanchored open-question self-resolution
(observed, benign here).
