---
prd: prd-verify-and-ship-skills
epic: agent-full-access
milestone: "0.3.0"
status: proposed
description: >
  Close the orchestrator loop with two focused skills: loopdeck:prd-verifier
  (read-only, verifies implemented code against a PRD's acceptance criteria
  with file:line evidence) and loopdeck:open-pr (pre-flight checks, generates
  a PR body from .loopdeck memory, runs gh pr create after user confirmation).
  Wire both into the orchestrator: a new Phase 6 Verify Against PRD, and a
  final Decide & Open PR step. Delivers the "the agent verifies itself
  against the spec and ships" half of the agent-full-access epic.
---

# PRD — PRD Verifier + Open PR Skills + Orchestrator Wiring

## Overview

Ship two focused, independently-invocable skills and wire them into the
existing `loopdeck:orchestrator` flow so that the end of an orchestrated
feature is no longer "stage files for commit" but "verify against the spec,
then open a PR."

- **`loopdeck:prd-verifier`** — read-only skill that parses a PRD's acceptance
  criteria, identifies the changed files, and returns a per-criterion
  pass/fail table with `file:line` evidence. Reusable outside the orchestrator
  (a human can run `/loopdeck:prd-verifier docs/epics/foo/prd-bar.md` anytime).
- **`loopdeck:open-pr`** — skill that runs pre-flight checks, gathers
  `.loopdeck/` context, generates a PR body, shows it to the user for
  confirmation, then runs `gh pr create --web`.
- **Orchestrator wiring** — insert "Verify Against PRD" as a new Phase 6 that
  invokes `prd-verifier`, renumber the existing Phase 6 to Phase 7, and add a
  final "Decide & Open PR" step that invokes `open-pr` when the verify
  verdict is green.

This is the skills half of the `agent-full-access` epic. The runtime half
(full access permission tier) is `prd-full-access-tier.md`. The two halves
are independent: this PRD ships even if the tier lands in a later milestone,
because verify + ship are useful under `ConfirmChanges` too.

## Problem Statement

The orchestrator today ends at "Decide Next Phase"
(`.agents/skills/loopdeck-orchestrator/SKILL.md:281-292`). That final phase
is a four-row outcome table — all green → "report completion," minor issues →
"spawn targeted fix agents," major gaps → "return to the relevant phase,"
PRD gap → "flag to user." What it never does:

1. **Verify the implementation against the PRD's acceptance criteria.** Phase
   5's "Final Review" (`:272-277`) runs stack-specific code reviews
   (`go-code-review`, `ios-code-review`) — those check code quality, not
   whether the PRD's stated acceptance criteria are met. The closest the
   skill gets is a Phase 2 plan-template row "contract alignment check with
   the PRD" (`:145`) and a Phase 6 "PRD gap discovered → Flag to user" row
   (`:290`). Neither is a structured per-criterion pass/fail pass.
2. **Open a pull request.** Phase 5's last line (`:279-280`) says "After the
   final review pass, add changed file to commit" — that's a `git add` nudge,
   not a `git commit`, not a push, not a branch, not a `gh pr create`. Grep
   for "pull request", "PR", "gh", "push", "branch", or "fork" across the
   orchestrator skill: zero hits outside the PRD acronym itself.

So the user still has to eyeball the diff against the spec and run
`gh pr create` by hand. That's the gap between "the agent built something"
and "the agent shipped something reviewable." This PRD closes it.

The skills live under `.agents/skills/` (the ZCode-discovered, git-tracked
path). `.claude/skills/` is gitignored (`.gitignore:11`) and is the Claude
Code client's own copy; this PRD does not mirror there. Per the managed-skills
ADRs in `decisions.md:153` and the `support-project-management` epic, the
`loopdeck-` prefix marks these skills as app-managed and subject to overwrite
on a future app version bump — accepted because the skills are authored here,
not customized downstream.

**Stack coverage.** A core design constraint: the skills operate on whatever
project LoopDeck has imported — that project may be Go, Android (Gradle), PHP
(Composer), iOS (Xcode/Swift), Ruby, Python, Java/Kotlin (Maven/Gradle),
.NET, Elixir, Node, Rust, or any future stack. LoopDeck's own
`scanner.rs:9-20` detects only 7 marker families today, but the skills do
**not** depend on `scanner.rs` — they read marker files directly via `Bash`
and can cover a broader stack range. Nothing in either skill may hardcode a
single stack's tooling. The PRD verifier reads the PRD's stated acceptance
criteria verbatim (they may be stack-agnostic, like "users can place
orders"); the `open-pr` "Test plan" section is **inferred from detected
markers** via the table in the `open-pr` design below, never hardcoded.

## Goals

| Priority | Goal |
|--------|------|
| P0 | `loopdeck:prd-verifier` skill at `.agents/skills/loopdeck-prd-verifier/SKILL.md` — read-only, parses PRD acceptance criteria, returns per-criterion pass/fail table with `file:line` evidence |
| P0 | `loopdeck:open-pr` skill at `.agents/skills/loopdeck-open-pr/SKILL.md` — pre-flight, body generation, user confirmation, `gh pr create --web` |
| P0 | Orchestrator: new Phase 6 "Verify Against PRD" invoking `prd-verifier`, with a verdict table (PASS/PARTIAL/FAIL → proceed/rework/return) |
| P0 | Orchestrator: renumber existing Phase 6 → Phase 7 "Decide & Open PR", with a green-verdict branch that invokes `open-pr` |
| P0 | Orchestrator: ASCII flow diagram updated to show the new Phase 6 + renumbered Phase 7 |
| P1 | Orchestrator: Phase 2 plan-template final-phase row updated to reference verify + ship |
| P1 | Orchestrator: Memory Convention cross-references updated from "Phase 6" to "Phase 7" where they refer to the final phase |
| P2 | Smoke-test checklist for both skills on a throwaway branch |

## Non-Goals

- **Enforcing that `open-pr` only runs after `prd-verifier` passes.** The two
  skills are independently invocable. The orchestrator wires them in sequence;
  a human calling `/loopdeck:open-pr` directly is trusted to have verified.
- **Auto-merging or auto-deploying PRs.** `open-pr` runs `gh pr create`; the
  human reviews and merges.
- **Verifying non-functional requirements** (performance, load, a11y, security
  audit). The verifier checks the PRD's stated acceptance criteria; NFRs are
  out of scope unless the PRD lists them as criteria.
- **AI-generated PRD drafts or phase decomposition.** That's 0.4.0 per the
  `support-project-management` epic. This PRD's verifier *parses* a PRD that
  already exists; it does not author one.
- **Mirroring the new skills to `.claude/skills/`.** That directory is
  gitignored and is the Claude Code client's own copy. ZCode reads
  `.agents/skills/`; that is the canonical, git-tracked path.
- **Rewriting the orchestrator from scratch.** The two new phases are
  surgical insertions; the rest of the orchestrator is unchanged.

## Design

### `loopdeck:prd-verifier` skill

**Location:** `.agents/skills/loopdeck-prd-verifier/SKILL.md`

**Frontmatter** (matches the orchestrator's schema —
`.agents/skills/loopdeck-orchestrator/SKILL.md:1-6`):

```yaml
---
name: loopdeck:prd-verifier
description: Verify implemented code against a PRD's acceptance criteria. Use after implementing a feature, when the user says "verify against PRD", "check acceptance criteria", "does this match the spec", or points to a PRD and the changed files. Returns a per-criterion pass/fail report with file:line evidence. Read-only — never edits files.
argument-hint: <prd-file-path>
allowed-tools: [Read, Glob, Grep, Bash]
---
```

`allowed-tools` deliberately omits `Edit`/`Write`/`Agent` — the verifier is
read-only and does not spawn sub-agents (epic ADR-4). `Bash` is included only
for `git diff --name-only` / `git log`; the skill must not run mutating git
commands.

**Body flow:**

1. **Parse the PRD.** Read the PRD at `$ARGUMENTS`. Extract explicit
   acceptance criteria — look for `## Success Criteria`, `## Goals` P0 rows,
   `## Acceptance Criteria`, or numbered "must" statements. If none are
   labeled, synthesize criteria from the user stories / Goals table and flag
   that to the user before proceeding ("no labeled acceptance section;
   inferred criteria: …").
2. **Identify changed files.** `git diff --name-only main...HEAD` when a
   feature branch is checked out; fall back to `git status --porcelain` when
   on `main`. Filter to source files per the project's stack: the skill
   detects marker files (see the `open-pr` table below) and uses each stack's
   conventional ignore set (`target/` for Rust, `node_modules/` for Node,
   `build/`/`.gradle/` for Android/JVM, `vendor/` for PHP/Go, `DerivedData/`
   for iOS, `__pycache__/` for Python, `_build/`/`deps/` for Elixir, etc.).
   `Glob` for `.gitignore` if present and prefer its rules.
3. **Per-criterion check.** For each criterion: locate supporting code via
   `Grep`/`Read`; state PASS / PARTIAL / FAIL with `file:line` evidence and a
   short code quote. PARTIAL means the criterion is partially satisfied (e.g.,
   happy path works but an edge case is missing). The criteria themselves are
   verbatim from the PRD — stack-agnostic when the PRD is (e.g. "users can
   place orders") and stack-specific when the PRD is (e.g. "the `/health`
   handler returns 200"). The skill makes no assumptions about which stack
   the project uses.
4. **Non-goals audit.** Read the PRD's `## Non-Goals` section. Flag any
   changed file or symbol that appears to implement a non-goal (scope creep).
5. **Report.** Render a markdown table:

   ```markdown
   ## PRD Verification — <prd filename>

   **Verdict:** PASS | WARN | BLOCK

   | # | Criterion | Status | Evidence |
   |---|-----------|--------|----------|
   | 1 | <criterion> | PASS | `src/foo.rs:42` — `<quote>` |
   | 2 | <criterion> | PARTIAL | `src/bar.rs:10` handles happy path; edge case X missing |
   | 3 | <criterion> | FAIL | no supporting code found |

   ### Non-goals audit
   - No scope creep detected. | Scope creep: `<file>` implements `<non-goal>`.
   ```

   Roll-up rule: any FAIL → BLOCK; any PARTIAL → WARN; all PASS → PASS.

6. **No edits.** The skill never modifies files. Output is the report only.

### `loopdeck:open-pr` skill

**Location:** `.agents/skills/loopdeck-open-pr/SKILL.md`

**Frontmatter:**

```yaml
---
name: loopdeck:open-pr
description: Open a pull request from the current branch using gh pr create. Use when the user says "open a PR", "create pull request", "ship this", or when an orchestrated feature is complete and verified. Runs pre-flight checks, generates a PR body from .loopdeck memory, links the PRD, shows the body for confirmation, then runs gh pr create --web.
allowed-tools: [Read, Bash, Grep]
---
```

`Bash` is required for `gh`, `git`, and writing the body to a temp file. No
`Edit`/`Write` tools — file writes go through `Bash` to a tmpfile so the skill
is explicit about the one artifact it touches. No `Agent` — the skill runs
synchronously and confirms with the user directly (epic ADR-5).

**Body flow:**

1. **Pre-flight checks** (abort cleanly with a remediation hint on failure):

   - `gh auth status` — abort with "run `gh auth login` first" if not
     authenticated.
   - `git rev-parse --abbrev-ref HEAD` — abort if `main` or `master` ("create
     a feature branch first: `git switch -c feat/...`").
   - `git rev-parse --abbrev-ref --symbolic-full-name @{u}` — abort if no
     upstream ("push first: `git push -u origin HEAD`").

2. **Gather context** (read-only):

   - `git log main..HEAD --oneline` — commit list for the "What changed"
     section.
   - Read `.loopdeck/decisions.md` — most recent 3 entries (by date) for the
     "Decisions" section.
   - Read `.loopdeck/loops.md` `## Current` — for the "Summary" section.
   - `git diff --stat main...HEAD` — for the high-level shape.

3. **Generate PR body** (markdown template, filled in by the skill):

   ```markdown
   ## Summary
   <one paragraph from loops.md ## Current>

   ## What changed
   - <commit subject>
   - <commit subject>

   ## PRD
   <relative path to the PRD, or "N/A" if not orchestrator-driven>

   ## Decisions
   - **<date> — <title>** (<status>): <one-line consequence>

   ## Test plan
   <stack-inferred checklist — see marker → test command table below>
   - [ ] Manual: <inferred from the PRD's success criteria>
   ```

   **Marker → test command inference table.** The skill `Glob`s the project
   root for marker files and emits the corresponding checklist. Unknown
   stacks produce a single "Run the project's test suite" line. The skill
   never hardcodes a stack:

   | Marker | Test command | Lint command |
   |--------|--------------|--------------|
   | `go.mod` | `go test ./...` | `go vet ./...` |
   | `Cargo.toml` | `cargo test` | `cargo clippy -D warnings` |
   | `package.json` (script `test`) | `npm test` (or `pnpm test` if `pnpm-lock.yaml`) | `npx tsc --noEmit` if TS, else skip |
   | `build.gradle`/`build.gradle.kts` (Android/JVM) | `./gradlew test` | `./gradlew lint` |
   | `pom.xml` (Maven) | `mvn test` | `mvn verify` |
   | `composer.json` (PHP) | `composer test` (if script) or `vendor/bin/phpunit` | `composer lint` if script |
   | `Package.swift` (Swift) | `swift test` | — |
   | `*.xcodeproj`/`*.xcworkspace` (iOS/macOS) | `xcodebuild test -scheme <scheme>` | — |
   | `Gemfile` (Ruby) | `bundle exec rake test` (or `rspec`) | `bundle exec rubocop` if present |
   | `pyproject.toml`/`setup.py` (Python) | `pytest` (or `python -m pytest`) | `ruff check` if present |
   | `mix.exs` (Elixir) | `mix test` | — |
   | `requirements.txt` only | `pytest` | — |
   | None recognized | `Run the project's test suite` | — |

   Multiple markers in one repo (e.g. a Go backend + Node frontend) produce a
   combined checklist with one section per stack.

4. **Show the body to the user for confirmation.** This is the
   outward-facing-action gate (epic ADR-5). The skill prints the drafted body
   and asks: proceed / edit / abort. If edit, the user pastes a revised body
   or the skill opens the draft in the user's `$EDITOR`.

5. **Create PR** — write the confirmed body to a tmpfile, then:

   ```bash
   gh pr create --title "<subject>" --body-file <tmpfile> --web
   ```

   `--web` opens the created PR in the browser for a final human review.
   Report the returned PR URL.

6. **LoopDeck memory write** — append the PR URL to `.loopdeck/loops.md`
   `## Next Steps` (via a `Bash` heredoc, since the skill has no `Edit` tool):
   `- [ ] Review & merge: <PR URL>`. This keeps the loop's next-step checklist
   accurate without the skill needing the broader memory-write conventions.

### Orchestrator wiring (`.agents/skills/loopdeck-orchestrator/SKILL.md`)

The orchestrator today is a 6-phase flow (Read PRD → Phase Decomposition →
Parallel Build → Code Review → Stitch & Integration → Decide Next Phase).
Two surgical insertions:

**New Phase 6: "Verify Against PRD"** — between current Phase 5 (Stitch,
ends at `:277` + the `:279` commit nudge) and current Phase 6 (Decide, starts
at `:281`):

```markdown
## Phase 6: Verify Against PRD

Before opening a PR, verify the implemented code against the PRD's stated
acceptance criteria.

### Invoke the verifier

Call the `loopdeck:prd-verifier` skill with the PRD path from `$ARGUMENTS`.
The skill is read-only; it produces a per-criterion pass/fail table with
file:line evidence.

### Verdict & Actions

| Verdict | Action |
|---------|--------|
| PASS (all criteria green) | Proceed to Phase 7. |
| WARN (one or more PARTIAL) | Spawn targeted fix agents for the PARTIAL criteria, re-verify. Do not proceed to Phase 7 until WARN clears or the user explicitly accepts the partial. |
| BLOCK (one or more FAIL) | Return to Phase 3 for the failing scope. Do not ship. |
| Non-goals scope creep flagged | Surface to the user; let the user decide whether to retract the scope or amend the PRD. |
```

**Renumber existing Phase 6 → Phase 7: "Decide & Open PR"** — extends the
current Phase 6 outcome table (`:283-292`). After the existing rows, add a
green-verdict branch:

```markdown
### Open PR (green verdict only)

When the verify verdict is PASS and Phase 5's integration check is green:

1. Call `loopdeck:open-pr`. The skill runs pre-flight checks, gathers
   `.loopdeck/` context, drafts a PR body, and shows it to the user for
   confirmation.
2. After the user confirms the body, the skill runs `gh pr create --web` and
   reports the PR URL.
3. Record the PR URL in `.loopdeck/loops.md ## Next Steps`.

Do NOT call `open-pr` on a WARN or BLOCK verdict. Ship only green work.
```

**Other orchestrator edits required for consistency:**

- **ASCII flow diagram** (`:14-41`) — add a Phase 6 box ("Verify Against PRD
  → invoke prd-verifier → PASS/PARTIAL/FAIL") and renumber the last box to
  Phase 7 ("Decide & Open PR → invoke open-pr on green").
- **Phase 2 plan template's final-phase row** (`:142-147`) — update the Phase
  6 row to "Verify Against PRD" and add a Phase 7 row "Decide & Open PR".
- **Phase 5's commit nudge** (`:279-280`, "After the final review pass, add
  changed file to commit") — keep the `git add`, but defer `git commit` to
  `open-pr` so the PR groups commits correctly and the commit message can be
  authored from the verified scope.
- **Memory Convention cross-references** — the orchestrator's
  "Integration with Phase 6" section (`:382-391`) and "Phase Actions"
  (`:376-381`) currently refer to the final phase as "Phase 6 or equivalent."
  Renumber to "Phase 7 or equivalent."

**Scope of edits:** all edits land in the canonical
`.agents/skills/loopdeck-orchestrator/SKILL.md` only. The `.claude/skills/`
mirror is gitignored and is not updated (per Non-Goals).

## Phases

### Phase 1 — `loopdeck:prd-verifier` skill

- [ ] Create `.agents/skills/loopdeck-prd-verifier/SKILL.md`
- [ ] Frontmatter: `name`, trigger-rich `description`, `argument-hint: <prd-file-path>`, `allowed-tools: [Read, Glob, Grep, Bash]`
- [ ] Body: parse → identify-changed-files → per-criterion-check → non-goals-audit → report flow
- [ ] Verdict roll-up rule documented (FAIL → BLOCK, PARTIAL → WARN, all PASS → PASS)
- [ ] Explicit "no edits" rule in the body
- [ ] Smoke test: invoke `/loopdeck:prd-verifier docs/PRD.md` on a known-complete feature; confirm a per-criterion table renders

### Phase 2 — `loopdeck:open-pr` skill

- [ ] Create `.agents/skills/loopdeck-open-pr/SKILL.md`
- [ ] Frontmatter: `name`, trigger-rich `description`, `allowed-tools: [Read, Bash, Grep]`
- [ ] Body: pre-flight → gather-context → generate-body → user-confirm → `gh pr create --web` → memory-write flow
- [ ] PR body template documented (Summary / What changed / PRD / Decisions / Test plan)
- [ ] Marker → test-command inference table embedded in the skill (Go, Rust, Node, Android/JVM, Maven, PHP, Swift, iOS, Ruby, Python, Elixir, unknown)
- [ ] Explicit user-confirmation gate before `gh pr create` (epic ADR-5)
- [ ] Smoke tests across stacks: on a throwaway Go repo (`go.mod`), confirm the Test plan emits `go test ./...`; on a Node repo (`package.json`), confirm `npm test`; on a repo with no recognized marker, confirm the "Run the project's test suite" fallback
- [ ] Smoke test on a throwaway branch: confirm pre-flight aborts on `main`, aborts on no-upstream, succeeds on a pushed feature branch

### Phase 3 — Orchestrator wiring

- [ ] Insert new "Phase 6: Verify Against PRD" section (invokes `prd-verifier`, verdict table)
- [ ] Renumber existing Phase 6 → "Phase 7: Decide & Open PR"; add the green-verdict `open-pr` branch
- [ ] Update ASCII flow diagram at `loopdeck-orchestrator/SKILL.md:14-41`
- [ ] Update Phase 2 plan-template final-phase rows at `:142-147`
- [ ] Update Phase 5 commit nudge at `:279-280` (keep `git add`, defer commit to `open-pr`)
- [ ] Renumber Memory Convention cross-references from "Phase 6" to "Phase 7" at `:376-391`
- [ ] Confirm no edits to `.claude/skills/` mirror (gitignored)

### Phase 4 — End-to-end smoke

- [ ] Run the orchestrator end-to-end on a small PRD; confirm the sequence produces: a verify report → a draft PR body → a created PR URL
- [ ] Confirm a BLOCK verdict from Phase 6 prevents the PR step
- [ ] Confirm a direct `/loopdeck:open-pr` invocation works without the orchestrator

## Open Questions

- **Stack coverage of the marker → test-command table.** The `open-pr` table
  lists 11 stacks. Should we ship all 11 in 0.3.0 or start with the 7 that
  LoopDeck's own `scanner.rs` detects (Rust, Node, Go, Swift, Ruby, iOS) and
  expand later? **Lean:** ship all 11 — the skill reads markers directly via
  `Bash`, doesn't depend on `scanner.rs`, and a missing stack just produces
  the "Run the project's test suite" fallback. Covering Go/Android/PHP/Maven
  now costs nothing and avoids a Stack-not-supported cliff for half the
  projects LoopDeck can import.
- Should `prd-verifier` support a "diff base" argument (e.g., verify against
  `origin/main` vs. a release tag), or always use `main...HEAD`? **Lean:**
  always `main...HEAD` in 0.3.0; add an optional second argument later if the
  release-flow case appears.
- Should `open-pr` auto-suggest squashing many tiny WIP commits, or leave
  commit grouping to the user? **Lean:** suggest in the PR body ("consider
  squash-merge: N commits") but never auto-execute. The human picks the merge
  strategy at merge time.
- Should the orchestrator's Phase 6 verify step re-run on every rework loop,
  or only at the end? **Lean:** every rework loop. The verdict is what tells
  the orchestrator to proceed vs. rework — verifying only at the end defeats
  the point. If the cost proves material, gate re-verification behind a
  PARTIAL verdict only.
- Should the skills be co-located with the orchestrator under
  `.agents/skills/loopdeck-orchestrator/` as separate files, or live in their
  own directories? **Lean:** own directories. The `SKILL.md` discovery
  convention is one skill per directory; co-location would require a
  multi-skill loader the harness doesn't have.
- Should `open-pr` write the PR URL into `decisions.md` as well as
  `loops.md`? **Lean:** `loops.md` only. A PR isn't an architectural decision;
  `decisions.md` is for ADRs. The PR URL is an execution artifact.
