---
name: loopdeck:prd-verifier
description: Verify implemented code against a PRD's acceptance criteria. Use after implementing a feature, when the user says "verify against PRD", "check acceptance criteria", "does this match the spec", or points to a PRD and the changed files. Returns a per-criterion pass/fail report with file:line evidence. Read-only — never edits files.
argument-hint: <prd-file-path>
allowed-tools: [Read, Glob, Grep, Bash]
---

# PRD Verifier — parse PRD → diff changed files → per-criterion check → non-goals audit → report

Verify the code on the current branch against a PRD's **stated acceptance
criteria** and render a per-criterion **PASS / PARTIAL / FAIL** report with
`file:line` evidence, plus a **non-goals scope-creep audit**. The skill is
**read-only** — it never edits, stages, commits, or spawns agents. Its sole
output is the report.

The skill is **stack-agnostic**: it reads the PRD's criteria verbatim (they may
be stack-agnostic like "users can place orders" or stack-specific like "the
`/health` handler returns 200") and detects the project's stack only to **filter
build artifacts out of the changed-file list**. It makes no assumptions about
which stack it is verifying and never hardcodes one stack's tooling.

The roll-up verdict is the gate the orchestrator's "Verify Against PRD" phase
consumes:

| Any FAIL | Any PARTIAL | All PASS | → | Verdict |
|---|---|---|---|---|
| yes | * | * | → | **BLOCK** |
| no | yes | * | → | **WARN** |
| no | no | yes | → | **PASS** |

## Full Flow

```
┌──────────────────────────────────────────────────────┐
│  1. Parse the PRD                                      │
│     $ARGUMENTS → acceptance criteria (verbatim);       │
│     synthesize from user stories/Goals if unlabeled    │
│     and flag that to the user first                    │
├──────────────────────────────────────────────────────┤
│  2. Identify changed files (read-only)                 │
│     git diff --name-only <default>...HEAD on a feature │
│     branch · git status --porcelain on the default     │
│     · filter per-stack build artifacts via the table   │
│     · prefer .gitignore                                │
├──────────────────────────────────────────────────────┤
│  3. Per-criterion check                                │
│     Grep/Read the changed set for each criterion →     │
│     PASS / PARTIAL / FAIL with `file:line` + quote     │
├──────────────────────────────────────────────────────┤
│  4. Non-goals audit                                    │
│     read PRD ## Non-Goals → flag any changed file or   │
│     symbol that implements a non-goal (scope creep)    │
├──────────────────────────────────────────────────────┤
│  5. Report                                             │
│     markdown table + verdict roll-up (no edits)        │
└──────────────────────────────────────────────────────┘
```

## Phase 1: Parse the PRD

### Step A: Resolve the PRD path

The PRD path comes from `$ARGUMENTS`.

- **Argument present** → use it. If the path does not exist (`test -f` fails),
  abort:
  > PRD not found at `<path>`. Pass the relative path to a PRD markdown file.
- **Argument absent** → look for the orchestrator's promote marker. Read
  `.loopdeck/loops.md` `## Current`; if a `Source:` / `**PRD**:` back-reference
  names a PRD file, use it. Otherwise abort:
  > No PRD path given. Re-run as `/loopdeck:prd-verifier <prd-file-path>`.

### Step B: Extract acceptance criteria

Read the PRD. Pull **explicit** acceptance criteria, in priority order:

1. A section literally titled `## Acceptance Criteria`, `## Success Criteria`,
   or `## Definition of Done`.
2. The **P0** rows of a `## Goals` table (the `P0` priority column).
3. Numbered / bulleted "must" / "shall" / "the system …" statements anywhere in
   the body.

Take the criteria **verbatim** — do not paraphrase, merge, or split them. The
report quotes them word-for-word; that is what makes the verdict auditable.

### Step C: Synthesize if nothing is labeled

If none of the above are present, **synthesize** criteria from the user stories
or the Goals table and **flag this to the user before proceeding**:

> No labeled acceptance section found in `<prd>`. Inferred criteria:
> 1. <inferred criterion>
> 2. <inferred criterion>
> Verifying against these inferred criteria. Correct me if any are wrong.

Synthesized criteria are marked `(inferred)` in the report so the verdict is
never mistaken for a check against the PRD's own words.

## Phase 2: Identify Changed Files (read-only)

The evidence search focuses on the code changed on this branch. No mutating git
commands — only `diff` / `log` / `status` / `rev-parse`.

### 2a. Resolve the diff base

Auto-detect the default branch so the skill works on `main` **or** `master`
repos:

```bash
default="$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null \
           | sed 's@^refs/remotes/origin/@@')"
[ -z "$default" ] && default=main
current="$(git rev-parse --abbrev-ref HEAD)"
```

### 2b. Get the changed file list

- **On a feature branch** (`$current` ≠ `$default`) → the committed diff:

  ```bash
  git diff --name-only "${default}...HEAD"
  ```

  The three-dot `...` uses the merge base, so only changes *unique to this
  branch* are listed — not everything since `$default` diverged elsewhere.

- **On the default branch** (`$current` = `$default`) → fall back to the working
  tree (there is no branch to diff against):

  ```bash
  git status --porcelain | sed 's/^...//'
  ```

  (`git status --porcelain` prefixes each line with XY status + a space; the
  `sed` strips the leading 3 chars to leave a plain path.)

If both are empty → abort:

> No changed files found on `<branch>` vs `<default>`, and a clean working tree.
> Implement the feature first, then verify.

### 2c. Filter build artifacts per stack (the stack-agnostic part)

`git diff --name-only` already excludes gitignored files that were never
tracked, so for the committed-diff path this filter is a safety net. It is
**load-bearing for the on-`default` working-tree fallback**, where
`git status --porcelain` lists untracked build artifacts too. Either way, drop
generated/vendored noise so evidence search stays on real source.

Detect the stack **at the repo root** via `ls` / `test -f` (Bash), then compose
a `grep -vE` filter from the matching ignore dirs. Multiple markers → union of
ignore sets (a Rust+Node repo filters `target/` **and** `node_modules/`).

| Marker(s) at repo root | Stack | Ignore dirs/patterns |
|---|---|---|
| `go.mod` | Go | `vendor/`, `bin/` |
| `Cargo.toml` | Rust | `target/` |
| `package.json` | Node | `node_modules/`, `dist/`, `build/`, `.next/` |
| `build.gradle` / `build.gradle.kts` | Android/JVM | `build/`, `.gradle/`, `out/` |
| `pom.xml` | Maven | `target/` |
| `composer.json` | PHP | `vendor/` |
| `Package.swift` | SwiftPM | `.build/` |
| `*.xcodeproj` / `*.xcworkspace` | iOS/macOS | `DerivedData/`, `build/` |
| `Gemfile` | Ruby | `vendor/bundle/`, `tmp/`, `coverage/`, `.bundle/` |
| `pyproject.toml` / `setup.py` / `requirements.txt` | Python | `__pycache__/`, `.venv/`, `venv/`, `*.egg-info/`, `.pytest_cache/`, `dist/` |
| `mix.exs` | Elixir | `_build/`, `deps/`, `cover/` |
| `*.sln` / `*.csproj` | .NET | `bin/`, `obj/` |
| _none recognized_ | unknown | _(no stack filter — rely on `.gitignore` + common noise)_ |

Always apply this **common-noise set** regardless of stack: `.DS_Store`,
`Thumbs.db`, `.idea/`, `.vscode/`.

Compose the filter **only from the detected stacks'** ignore dirs (not the full
table — a Node repo should not drop a legit `vendor/assets/` dir that merely
shares a name with Go's `vendor/` artifacts). Build the alternation by joining
the ignore-dir names of each detected stack with `|`, then anchor each as a path
component with `(^|/)...(/|$)` so `src/target.rs` is kept but `target/debug/…`
is dropped.

Worked example (Rust + Node repo, like this one — `Cargo.toml` + `package.json`):

```bash
# detected stacks: Rust (target/) + Node (node_modules/ dist/ build/ .next/)
stack_ignore='(^|/)(target|node_modules|dist|build|\.next)(/|$)'
common_ignore='(^|/)(\.idea|\.vscode)(/|$)|/(DS_Store|Thumbs\.db)$'
git diff --name-only "${default}...HEAD" \
  | grep -vE "$stack_ignore" \
  | grep -vE "$common_ignore"
```

(If no marker is recognized, set `stack_ignore` empty and apply only
`common_ignore`.)

### 2d. Prefer `.gitignore`

`Glob` for `.gitignore` at the repo root. If present, its rules are the source
of truth — any file it excludes that slipped into the list above is also dropped.
(In practice `git diff --name-only` already honors `.gitignore` for untracked
files, so this is belt-and-suspenders; do it anyway for the working-tree path.)

Keep the filtered list as the **changed-file set** for Phase 3. If it is empty
after filtering (only build artifacts changed) → report that explicitly rather
than FAILing every criterion:

> All changes are build artifacts (`<examples>`), filtered out. No source
> changes to verify against the PRD.

## Phase 3: Per-Criterion Check

For **each** criterion from Phase 1, locate supporting code and assign a status.

### 3a. Search strategy

Derive search terms from the criterion's own nouns/verbs (identifiers, endpoint
paths, type names, error strings, UI labels). Then:

1. **`Grep`** the **changed-file set first** for those terms (the feature under
   review must actually touch the relevant code).
2. **`Read`** the hits (with `file:line`) to confirm the code genuinely
   implements the criterion — not just a comment or a string mention.
3. Only if the criterion is about **integration/usage** of pre-existing code
   (e.g. "calls the existing auth layer"), broaden the `Grep` to the whole tree.

### 3b. Status assignment

| Status | When |
|---|---|
| **PASS** | The criterion is satisfied by code on this branch. Cite `file:line` + a short quote. |
| **PARTIAL** | The criterion is partly satisfied — typically the happy path works but a stated edge case, error path, or constraint is missing or unverified. Cite what works **and** what is missing. |
| **FAIL** | No supporting code found, or the code present contradicts the criterion. State what was searched and what was expected. |

Rules that keep the verdict honest:

- **Evidence over assertion.** Every PASS/PARTIAL cites a concrete `file:line`
  in a file that exists at `HEAD`. A criterion with no citable evidence is FAIL,
  never a generous PASS.
- **Changed-set priority.** Prefer evidence from the **changed-file set**. If a
  criterion's only supporting code lives in an **unchanged** file, that is fine
  for integration criteria, but **flag it** — the change under review does not
  itself satisfy the criterion, which is usually a PARTIAL, not a PASS.
- **No running code.** The skill does not build, run tests, or hit endpoints.
  Behavioral claims ("returns 200", "handles the error") are judged from the
  code's structure, not from execution. If a criterion can only be proven by
  running it, mark it PARTIAL and state "verify by running `<inferred command>`".
- **Verbatim criteria.** Quote the criterion text exactly as the PRD wrote it.

## Phase 4: Non-Goals Audit

Read the PRD's `## Non-Goals` section (if absent → state "No `## Non-Goals`
section; scope-creep audit skipped." and move on).

For each non-goal, scan the **changed-file set** for code that appears to
implement it. A non-goal is a scope-creep hit when a changed file/symbol clearly
delivers what the PRD explicitly excluded (e.g. the PRD says "no caching layer"
and the diff adds a `Cache` struct; the PRD says "iOS-only" and the diff adds an
Android module).

This is a **flag for the user**, not an automatic FAIL — non-goals are often
deliberate, in-progress, or a sign the PRD needs amending. Surface each hit with
the file and the non-goal it appears to cross.

## Phase 5: Report

Render exactly this structure (markdown). This is the skill's **only** output —
no files are written.

```markdown
## PRD Verification — <prd filename>

**Verdict:** PASS | WARN | BLOCK

| # | Criterion | Status | Evidence |
|---|-----------|--------|----------|
| 1 | <criterion verbatim> | PASS | `src/foo.rs:42` — `<short quote>` |
| 2 | <criterion verbatim> | PARTIAL | `src/bar.rs:10` — happy path; `<edge case>` missing |
| 3 | <criterion verbatim> | FAIL | searched `<terms>` in the changed set; no supporting code |

### Non-goals audit
- No scope creep detected.     ← or
- Scope creep: `<file>:<line>` implements `<non-goal>` ("…").
```

### Roll-up

Apply the roll-up rule strictly — it is the gate the orchestrator reads:

- **any FAIL** → `**Verdict:** BLOCK`
- else **any PARTIAL** → `**Verdict:** WARN`
- else → `**Verdict:** PASS`

State the roll-up once, in the `**Verdict:**` line, so a downstream reader (or
the orchestrator's verdict table) can grep a single token.

End the report with one line the orchestrator can key off:

> Roll-up: **BLOCK** / **WARN** / **PASS** — `<count FAIL>` FAIL, `<count
> PARTIAL>` PARTIAL, `<count PASS>` PASS.

## Important Rules

- **Read-only. Never edit, stage, commit, or push.** The skill has no
  `Edit`/`Write`/`Agent`. `Bash` is for `git diff` / `git log` / `git status` /
  `git rev-parse` / `ls` / `test` only — **no mutating git commands** (`add`,
  `commit`, `push`, `checkout`, `reset`, `stash`, `rebase`, …). If a step seems
  to need a mutation, stop and report instead.
- **Stack-agnostic.** The project's stack is detected only to filter build
  artifacts out of the changed-file list. The criteria come verbatim from the
  PRD and are never rewritten to match an assumed stack. A repo with no
  recognized marker still verifies — just with no stack-specific filter.
- **Evidence, not assertion.** Every PASS/PARTIAL cites a real `file:line` at
  `HEAD`. No evidence → FAIL. Behavioral criteria that need execution → PARTIAL
  with a "verify by running" note, never a PASS.
- **Verbatim criteria.** Quote the PRD's acceptance criteria word-for-word.
  Synthesized criteria are labeled `(inferred)` and disclosed up front.
- **Honest roll-up.** One FAIL blocks the whole verdict; one PARTIAL warns it;
  the verdict line and the roll-up line must agree. Do not soften a FAIL to a
  PARTIAL to make the report greener.
- **No sub-agents.** The skill runs synchronously. Do not spawn agents (epic
  ADR-4).
- **Diff base is the default branch.** Uses `<default>...HEAD` (merge base), with
  `main`/`master` auto-detected. No alternate-base argument in 0.3.0 — if a
  release-tag base is needed later, add an optional second argument then.
