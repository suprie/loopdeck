---
prd: prd-process-discipline
epic: optimization
milestone: "0.4.0"
status: proposed
description: >
  Write down and actually enforce a guardrail against process-about-process
  loops — reconciliation loops auditing prior reconciliation loops, PRD
  amendments about amendments — after a review found this pattern in recent
  loop history plus a near-miss where an agent almost deleted the
  orchestrator by misreading its own memory.
order: 50
---

# PRD — Process Discipline

## Overview

Recent `.loopdeck/loops.md`/`decisions.md` history shows a recurring
pattern: a loop reconciles stale checkboxes or PRD state, then a later loop
audits that reconciliation, then another amends the amendment. One incident
came within a misread of an agent deleting the orchestrator itself. This
PRD writes down a concrete, checkable rule against this pattern and wires
it somewhere it's actually consulted — not just another paragraph in a
memory file that this same pattern proves doesn't get re-read carefully.

## Problem Statement

- There's no written rule distinguishing "this loop fixes real drift" from
  "this loop exists because a prior loop's fix wasn't trusted." Without one,
  any agent (or the maintainer, tired) can justify another audit loop.
- Memory files documenting this problem (per `prd-memory-hygiene.md`) are
  themselves too large to reliably re-read, which is part of how the
  near-miss happened — an agent misread its own memory under load.
- No existing skill or hook checks for this pattern before a loop is
  promoted; it's caught only in hindsight, if at all.

## Goals

| Priority | Goal |
|----------|------|
| P0 | Write a concrete rule: what makes a loop "reconciliation-about-reconciliation" versus legitimate drift-fixing, with the recent real cases as examples. |
| P0 | Decide where the rule is enforced — `loopdeck-loop-runner` skill (checked before promoting/starting a loop) and/or the Stop hook — and wire it there, not only into a memory file. |
| P1 | Dry-run the rule against the actual reconciliation-loop cases already in `loops.md`/`decisions.md` history to confirm it would have caught them. |
| P2 | Add a lightweight check specifically for self-destructive misreads (e.g. a confirmation step before an agent deletes/rewrites orchestrator-critical files based on its own memory read). |

## Non-Goals

- A general project-management process overhaul — this is one specific
  guardrail, not a new planning methodology.
- Blocking all audit/reconciliation loops outright — some are legitimate;
  the rule must distinguish, not prohibit.
- Building new tooling beyond what's needed to wire the rule into an
  existing skill or hook.

## Design

_Stub — whether this lands as a loop-runner precondition, a Stop-hook
check, or both is a Phase 1 decision made against the real historical
cases, not decided here in advance._

## Phases

### Phase 1 — Define the rule

- [ ] Pull the specific reconciliation-loop and near-miss cases from
      `loops.md`/`decisions.md` history and state, in one paragraph each,
      what distinguished them from legitimate drift-fixing (or didn't).
- [ ] Draft a concrete, checkable rule from those cases — a short
      checklist or a single disqualifying question a loop must pass before
      it's promoted.

### Phase 2 — Wire it in

- [ ] Add the rule to `loopdeck-loop-runner` (or the Stop hook, per the
      Phase 1 decision) as an actual precondition check, not prose in a
      memory file.
- [ ] Add the self-destructive-misread check (confirm before an
      orchestrator-critical file is deleted/rewritten based on a memory
      read) if Phase 1's cases show it's warranted.

### Phase 3 — Verification

- [ ] Dry-run the wired check against the historical cases from Phase 1;
      confirm it would have flagged them.
- [ ] Confirm the check doesn't false-positive on a known-legitimate
      drift-fixing loop from history.

## Open Questions

- Does this rule live in the loop-runner skill, the Stop hook, or both?
  Resolve in Phase 1 against the actual failure cases — don't presuppose
  the mechanism before the cases are examined.
