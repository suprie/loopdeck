---
prd: prd-non-code-artifacts
epic: role-based-orchestration
milestone: "0.6.0"
status: proposed
description: >
  Artifact types beyond code branches: handoff artifacts gain a type
  dimension (code-branch, doc, content), and doc/content phases run without
  a git worktree or PR requirement — writing reviewed artifacts through
  review gates instead of test gates, so marketing and business-analyst
  roles produce visible, gated work.
---

## Overview

The orchestration infra is code-shaped: sessions are keyed by project path
or worktree, and run outputs are branches and draft PRs. This PRD gives
handoff artifacts a type model and lets doc/content phases execute without
the worktree/PR machinery, gated by review (prd-arbitration) instead of
tests. Target demo: BA brief → marketing draft → decision-maker review
gate → parked human approval.

## Problem Statement

A marketing or business-analyst role has nothing to produce inside the
current pipeline: no artifact type represents a brief or a content draft,
every phase assumes a code checkout, and verification assumes
`prd-verifier` against code. Without non-code artifacts, half the roles
this epic exists to staff cannot participate.

## Goals

| Priority | Goal |
|---|---|
| P0 | Artifact type dimension on handoff artifacts (code-branch, doc, content) with per-type output contracts |
| P0 | Doc/content phase execution without a worktree or PR requirement |
| P1 | Review-gated flow for content: author role drafts, decision-maker reviews, human approves at the escalation point |
| P1 | End-to-end BA-brief → marketing-draft → review → parked-approval demo |

## Non-Goals

- Publishing integrations (no CMS, social, or email targets — artifacts land in the repo/filesystem only)
- Binary asset generation (copy and structured docs only, for now)
- Changing how code-branch phases work — the type model must leave them untouched
- Multimodal outputs (images, video)

## Design

Stub — points to resolve while implementing:

- Do content artifacts stay git-backed (branch + draft PR carrying the doc)
  or bypass git entirely into a content directory? Git-backed keeps the
  audit trail nearly free but forces a worktree; bypass is lighter but
  loses history
- Where content outputs live on disk per project (`.loopdeck/` store vs a
  project-visible folder the user picks at plan creation)
- How `prd-verifier` acceptance criteria generalize to non-code checks
  (structure present, sources cited, tone contract met)

## Phases

### Phase 1 — Artifact type model

- [ ] `non-code-artifacts/type-model` Give handoff artifacts a type dimension (code-branch, doc, content) with per-type output contracts

### Phase 2 — Doc/content pipelines

- [ ] `non-code-artifacts/doc-pipeline` Let doc and content phases run without a git worktree or PR requirement, writing reviewed artifacts with review gates instead of test gates
- [ ] `non-code-artifacts/review-flow` Verify the BA-brief to marketing-draft to decision-maker-review to parked-human-approval flow end-to-end

### Phase 3 — Tests/verification

- [ ] `non-code-artifacts/tests` Add Rust tests for type routing and pass prd-verifier against the BA/marketing demo plan

## Open Questions

- Git-backed or git-bypassed content — which is the default, and is it per-phase?
- Should doc/content phases count against token budgets the same way code phases do?
- Who owns the content directory layout — the plan author or the role charter's output contract?
