---
title: Role Demo Run Record
epic: role-based-orchestration
prd: prd-role-foundations
loop: role-foundations/role-demo
status: completed
created: 2026-09-04
---

# Role Demo Run Record (dev builds / QA verifies)

Observed behavior for the `prd-role-foundations` Phase 4 loop
`role-foundations/role-demo`: a two-phase plan where a dev-role agent builds
and a QA-role agent verifies, with per-role attribution in the run report.

## Setup

| | |
|---|---|
| Fixture repo | Scratch temp repo (own `.loopdeck/`, own `docs/epics/role-demo/` PRD, `git init`) — no pollution of loopdeck's real docs/epics or execution.yaml |
| Loop specs | Two authored checklist items: `role-demo/dev-build` (create `math.js` + `math.test.js`, make `node math.test.js` pass) and `role-demo/qa-verify` (re-run tests, review, write `qa-report.md` ending `**QA verdict:** PASS`) |
| Roster entries | Temp `Dev Role (demo)` + `QA Role (demo)` entries in the real global registry, created at demo start and removed after the report was captured (Drop-guard snapshot restore; registry verified clean post-run). Demo re-runs recreate them |
| Executor primitives | The executor's exact path: `next_queued_batch` (split by assigned agent), `resolve_agent_config_by_id`, `start_fresh_and_record_streaming_in_root_with_config` — real claude sessions, not mocks |
| Operator deviation | Per the loop's pre-answered clarification, full `execute_run` is not drivable headless (needs `AppHandle`); the demo drives the executor's own primitives in a `#[ignore]`d test (`role_demo_tests.rs`), run explicitly |
| Prompt deviation | The turn prompt is demo-authored rather than `build_combined_phase_prompt` — the production prompt mandates the verify→ship / draft-PR flow, which a remoteless scratch repo must not attempt. The assignment machinery under test (batch split, roster resolution, charter-carrying spawn, plan/report attribution) is exactly the executor's |
| Budgets | 300k token cap per phase turn (demo-local; production caps come from the plan's `RunBudgets`) |
| Sample size | 1 run, `cargo test role_demo -- --ignored --nocapture`, 66.5s wall |

## Per-role attribution (from `.loopdeck/role-demo-report.md` in the fixture)

| Phase | Loop | Agent | Verdict | Tokens | Wall |
|---|---|---|---|---|---|
| role-demo/dev-build | `role-demo/dev-build` | Dev Role (demo) | Pass | 97,620 | 33s |
| role-demo/qa-verify | `role-demo/qa-verify` | QA Role (demo) | Pass | 58,018 | 31s |

## Observed behavior notes

1. **Batch split held**: each phase ran as its own single-phase batch under
   its own agent config — the dev phases and QA phase never shared a turn.
2. **Charters rode the config**: `resolve_agent_config_by_id` copied each
   roster entry's charter into the spawn; both sessions complied (dev: 43-byte
   `math.js`, minimal increment, no verification beyond its own test run; QA:
   re-ran the tests, reviewed without modifying, report ends with the literal
   `**QA verdict:** PASS` line its output contract demands).
3. **Report attribution is per-phase**: `RunReport::from_plan` carried each
   phase's `assigned_agent` (dev id / qa id respectively), both verdicts Pass.
4. **Registry hygiene**: snapshot restore removed both temp entries; the
   global registry was never created (it didn't exist) and post-run inspection
   found zero `(demo)` entries.

## Go/No-Go

**GO** — dev built exactly the increment, QA verified without building,
attribution is per-role in the report, registry left untouched.
