---
name: loopdeck:open-pr
description: Ship the current branch as a reviewable pull request. Use when the user says "open a PR", "create pull request", "ship this", "ready to ship", or when an orchestrated feature is complete and verified. Runs pre-flight checks (gh auth, feature branch, remote origin), gathers context from .loopdeck memory + git log + working-tree status, generates a PR body (Summary / What changed / PRD / Decisions / Test plan) with the Test plan inferred from the project's stack markers, shows the body for confirmation, then — only after the user confirms — commits any uncommitted work (message authored from the verified scope), pushes, and runs gh pr create --web. No commit, push, or publish before the body confirmation.
allowed-tools: [Read, Bash, Grep]
---

# Open PR — pre-flight → draft body → confirm → commit + push → gh pr create --web

Open a reviewable pull request for the work on the current branch. The skill is
**stack-agnostic**: it infers the Test plan from the project's build markers, so
it works on a Go, Rust, Node, Android/JVM, Maven, PHP, Swift, iOS, Ruby, Python,
or Elixir project — never hardcoded to one stack. LoopDeck imports any of those,
so the skill must not assume which one.

This skill is the **auto-commit hook point** for an orchestrated feature: the
orchestrator builds and verifies the work; `open-pr` owns the stage → commit →
push → publish tail. The commit message is authored from the **verified scope**
(the same title + Summary the user confirms in the PR body), so the commit and
the PR describe the feature identically.

Two outward-facing actions — `git push` and `gh pr create` — are both gated
behind a single **user confirmation** of the drafted PR body (Phase 4). Before
that confirmation the skill is entirely read-only; nothing is staged, committed,
pushed, or published. After confirmation it commits any uncommitted work, pushes,
and opens the PR.

## Full Flow

```
┌──────────────────────────────────────────────────────┐
│  1. Pre-flight                                         │
│     gh auth · not on main/master · remote origin       │
│     (abort with a one-line remediation on failure)     │
├──────────────────────────────────────────────────────┤
│  2. Gather context (read-only)                         │
│     git log main..HEAD · git diff --stat ·             │
│     git status --porcelain · .loopdeck/decisions.md ·  │
│     loops.md ## Current                                │
├──────────────────────────────────────────────────────┤
│  3. Generate PR body                                   │
│     Summary / What changed / PRD / Decisions /         │
│     Test plan (marker-inferred) + commit message       │
│     (= title + Summary, from the verified scope)       │
├──────────────────────────────────────────────────────┤
│  4. Show body → user confirms (proceed/edit/abort)     │
│     (the gate for commit + push + publish; discloses   │
│     any uncommitted files + the commit message)        │
├──────────────────────────────────────────────────────┤
│  5. Commit uncommitted work + push (gated by Phase 4)  │
│     git add -A → git commit (if dirty) → push -u       │
├──────────────────────────────────────────────────────┤
│  6. gh pr create --title … --body-file … --web         │
├──────────────────────────────────────────────────────┤
│  7. Append PR URL to .loopdeck/loops.md ## Next Steps  │
└──────────────────────────────────────────────────────┘
```

## Phase 1: Pre-flight Checks

Run each check in order; abort on the first failure with the exact remediation.
Do not proceed past a failed check. All three are read-only.

### 1a. `gh` is authenticated

```bash
gh auth status
```

- **Exit 0** → authenticated; continue.
- **Non-zero** → abort:
  > `gh` is not authenticated. Run `gh auth login` first, then re-run this skill.

### 1b. Not on the default branch

```bash
git rev-parse --abbrev-ref HEAD
```

- If the output is `main` or `master` → abort:
  > You are on `main`. Create a feature branch first: `git switch -c feat/<short-description>`.
- Otherwise → remember the branch name; continue.

### 1c. Remote `origin` exists

This skill **pushes** the branch (Phase 5), so a remote is required — unlike a
skill that only opens a PR on already-pushed work. We do **not** require the
upstream to be pushed yet, because pushing is this skill's job.

```bash
git remote get-url origin
```

- **Exit 0** → `origin` is configured; continue.
- **Non-zero** → abort:
  > No `origin` remote. Add one (`git remote add origin <url>`) before shipping.

## Phase 2: Gather Context (read-only)

Collect everything the body and the commit need. No mutating git commands here.

```bash
# Commits unique to this branch (subjects feed "What changed")
git log main..HEAD --oneline

# High-level shape of the committed diff
git diff --stat main...HEAD

# Uncommitted changes that Phase 5 will fold into the ship commit.
# This is what makes the commit honest: the body (Phase 3) and the confirm
# gate (Phase 4) disclose these, so the user sees exactly what gets committed.
git status --porcelain
```

Then read:

- `.loopdeck/decisions.md` — take the **3 most recent** entries (by the date in
  the `## YYYY-MM-DD — Title` heading) for the **Decisions** section.
- `.loopdeck/loops.md` → `## Current` — the **Goal** text feeds the **Summary**,
  and any `**PRD**:` / `**Epic**:` / `Source:` back-reference (the orchestrator
  writes these on promote) feeds the **PRD** section.

If `.loopdeck/` does not exist (not a LoopDeck-tracked project), the Summary,
Decisions, and PRD sections degrade to commit-derived content and an `N/A` PRD —
the skill still works.

### Nothing-to-ship guard

If `git log main..HEAD --oneline` is empty **and** `git status --porcelain` is
empty → abort:

> Nothing to ship: no commits ahead of `main` and a clean working tree.

(If the default branch is `master`, mentally substitute it for `main` throughout.)
Otherwise continue — there is either committed work on the branch or uncommitted
work in the tree (or both) to ship.

## Phase 3: Generate the PR Body

Fill this template. Keep it honest: do not invent tests that were run or
decisions that were not made. The Test plan is a *checklist for the reviewer*,
not a claim of what passed.

```markdown
## Summary
<one paragraph — from loops.md ## Current Goal, or a synthesis of the commit
subjects if there is no .loopdeck memory>

## What changed
- <commit subject>
- <commit subject>
- _(collapse near-duplicate subjects; keep the list scannable)_
- _(if `git status --porcelain` is non-empty, add: "Plus N uncommitted file(s) \
committed by this skill before push: <list>" so the reviewer sees the full scope)_

## PRD
<relative path to the PRD from loops.md ## Current, e.g.
docs/epics/<slug>/prd-<name>.md — or `N/A` if not orchestrator-driven>

## Decisions
- **<YYYY-MM-DD> — <title>** (<status>): <one-line consequence>
- **<YYYY-MM-DD> — <title>** (<status>): <one-line consequence>
- _(most-recent 3 from .loopdeck/decisions.md; omit the whole section if none)_

## Test plan
<stack-inferred checklist from the table below>
- [ ] Manual: <the PRD's top success criterion, or "verify the feature end-to-end">
```

### Title

Derive the PR title from the **branch name**: strip a leading type prefix
(`feat/`, `fix/`, `chore/`, `docs/`, `refactor/`, `build/`), replace `-` and `_`
with spaces, and Title Case it. Example: `feat/project-management` →
`Project Management`. The user can edit it at the confirmation gate. Fallback if
the branch is opaque: the oldest commit subject in the range
(`git log main..HEAD --format=%s | tail -1`).

### Commit message (for Phase 5)

The ship commit's message is authored from the **same verified scope** as the
body — it is not a separate artifact:

- **Subject**: the derived PR title (above).
- **Body**: the **Summary** paragraph verbatim.

So the commit and the PR describe the feature identically, and the user confirms
both at the Phase 4 gate. If the working tree is clean (everything already
committed in earlier phases), Phase 5 makes **no** new commit — it skips straight
to push.

### Test plan — marker → command inference

Detect build markers **at the repository root** via `ls` / `test -f` (Bash — the
skill has no `Glob` tool). Emit the matching checklist lines. A repo with
**multiple** markers (e.g. a Go backend + Node frontend) emits one block per
detected stack. **Never hardcode a stack** — if no marker is recognized, emit the
fallback line.

| Marker(s) at repo root | Test command | Lint command (append if present) |
|---|---|---|
| `go.mod` | `go test ./...` | `go vet ./...` |
| `Cargo.toml` | `cargo test` | `cargo clippy -- -D warnings` |
| `package.json` | `npm test` (use `pnpm test` if `pnpm-lock.yaml` exists; `yarn test` if `yarn.lock` exists) | `npx tsc --noEmit` when `tsconfig.json` is present |
| `build.gradle` / `build.gradle.kts` | `./gradlew test` | `./gradlew lint` |
| `pom.xml` | `mvn test` | `mvn verify` |
| `composer.json` | `composer test` (or `vendor/bin/phpunit` if no `test` script) | `composer lint` if a `lint` script exists |
| `Package.swift` | `swift test` | — |
| `*.xcodeproj` / `*.xcworkspace` | `xcodebuild test -scheme <scheme>` | — |
| `Gemfile` | `bundle exec rake test` (or `bundle exec rspec`) | `bundle exec rubocop` if `.rubocop.yml` is present |
| `pyproject.toml` / `setup.py` | `pytest` (or `python -m pytest`) | `ruff check .` if a `ruff` config is present |
| `mix.exs` | `mix test` | — |
| `requirements.txt` (only) | `pytest` | — |
| _none recognized_ | `Run the project's test suite` | — |

Render the detected entries as a checklist, one block per stack:

```markdown
## Test plan
- [ ] `go test ./...`
- [ ] `go vet ./...`
- [ ] Manual: <PRD success criterion>
```

### `package.json` detail

For Node: confirm a `test` script exists (`jq -e '.scripts.test' package.json`
or `grep '"test"' package.json`) before emitting `npm test`; if there is no test
script, emit `Run the project's test suite` instead. Detect `pnpm-lock.yaml` →
prefer `pnpm`; `yarn.lock` → prefer `yarn`.

## Phase 4: Show the Body for Confirmation (the commit + push + publish gate)

**Print the complete drafted PR body** (in a fenced code block), the derived
title, and — if `git status --porcelain` (Phase 2) is non-empty — an explicit
**disclosure** of:

- the uncommitted files that Phase 5 will commit, and
- the commit message that will be used (the title + the Summary paragraph).

Then explicitly ask the user:

> Drafted PR — proceed, edit, or abort?

- **proceed** → continue to Phase 5 (commit + push), then Phase 6 (`gh pr create`).
  The body confirmation authorizes the commit, the push, and the publish in a
  single gate — nothing is committed, pushed, or published before the user says
  proceed.
- **edit** → accept a revised body from the user, or open the draft in their
  `$EDITOR` (`"${EDITOR:-vi}" <tmpfile>`), then re-confirm the edited body
  before proceeding.
- **abort** → stop. No commit, no push, no publish.

Do **not** stage, commit, push, or call `gh pr create` until the user says
proceed. This gate is the only thing standing between a drafted-but-unreviewed
body and a commit + push + public publish.

## Phase 5: Commit Uncommitted Work + Push (gated by Phase 4)

Runs **only after** the user confirms the body in Phase 4. Two steps: fold any
uncommitted work into one coherent commit (message from the verified scope), then
push. This is where an orchestrated feature's WIP becomes a pushable, PR-able
branch tip.

### 5a. Stage + commit (only if the working tree is dirty)

```bash
# Stage the whole feature — the body the user just confirmed lists this scope.
git add -A

# Commit only if something is staged; skip cleanly if the tree was already clean.
if ! git diff --cached --quiet; then
  git commit -m "<derived title>" -m "<Summary paragraph verbatim>"
fi
```

- The commit message (title + Summary) is the verified scope the user just
  confirmed in the body, so the commit and the PR describe the feature
  identically.
- If the working tree was already clean (everything committed in earlier phases),
  `git diff --cached --quiet` is true and the commit is skipped — Phase 5 just
  pushes the existing commits.
- **No squash, no history rewrite.** Existing WIP commits on the branch are
  pushed as-is. The human picks the merge strategy (squash-merge vs. merge) at
  merge time; this skill never rewrites published history. (If there are many
  tiny WIP commits, the body may *suggest* "consider squash-merge: N commits" as
  a reviewer note — it never executes it.)

### 5b. Push

```bash
git push -u origin HEAD
```

- `-u origin HEAD` sets the upstream on the first push and is a no-op for the
  upstream binding on subsequent pushes, so it works whether or not the branch
  was pushed before.
- This is the only outward-facing action before `gh pr create`, and it is gated
  by the Phase 4 body confirmation. If the push fails (e.g. rejected non-fast
  -forward), stop and report — do **not** force-push and do **not** proceed to
  `gh pr create`.

## Phase 6: Create the PR

The branch is now committed and pushed (Phase 5), so the upstream exists. Write
the **confirmed** body to a temp file, then open the PR. Capture the URL `gh`
prints.

```bash
tmpfile="$(mktemp)"   # portable: $TMPDIR on macOS, /tmp on Linux
cat > "$tmpfile" <<'BODY'
<confirmed PR body verbatim>
BODY

gh pr create --title "<derived title>" --body-file "$tmpfile" --web
status=$?
rm -f "$tmpfile"
exit $status
```

`--web` opens the browser with the title and body **pre-filled** (passed as query
params), so the human has a final review click before the PR is actually created.

> **Note on `--web` and the URL:** with `--web`, GitHub does **not** create the
> PR until the human clicks *Create pull request* in the browser, and `gh`
> prints the pre-filled compare URL rather than a final `…/pull/N` URL — that
> compare URL is what Phase 7 records. If the user prefers the PR created
> headlessly with a real `…/pull/N` URL captured automatically, drop `--web`
> (`gh pr create --title … --body-file "$tmpfile"` returns the PR URL on stdout)
> and optionally open it with `gh pr view --web`.

Report the URL `gh` returned to the user.

## Phase 7: Record the PR URL in `.loopdeck/loops.md`

Append a review/merge checklist item to the `## Next Steps` section. The skill
has no `Edit`/`Write` tool by design, so this is a Bash insert — place the new
line immediately under the `## Next Steps` heading so it is the most visible next
step. This `awk` form is portable across macOS/BSD and GNU:

```bash
url="<the URL gh returned>"
line="- [ ] Review & merge: $url"
awk -v l="$line" '/^## Next Steps/ { print; print l; next } { print }' \
  .loopdeck/loops.md > .loopdeck/loops.md.tmp \
  && mv .loopdeck/loops.md.tmp .loopdeck/loops.md
```

If `.loopdeck/loops.md` has no `## Next Steps` section, append one:

```bash
printf '\n## Next Steps\n%s\n' "$line" >> .loopdeck/loops.md
```

If `.loopdeck/` does not exist, skip this phase silently — the PR was still
created and reported in Phase 6.

## Important Rules

- **Stack-agnostic.** The Test plan is *inferred* from markers, never hardcoded.
  A missing marker yields the generic "Run the project's test suite" line —
  never a wrong command.
- **Commit + push are owned here, gated by the body confirmation.** Phases 1–4
  are read-only (`gh auth status`, `git log`, `git diff`, `git status`,
  `git rev-parse`, `git remote`). Phase 5 is the only mutating phase: it stages
  (`git add -A`), commits uncommitted work with a message authored from the
  verified scope (the confirmed title + Summary), and pushes (`git push -u origin
  HEAD`). It runs **only after** the user confirms the body in Phase 4 — no
  staging, no commit, no push, and no `gh pr create` before that. This is the
  **auto-commit hook point**: the orchestrator builds and verifies; `open-pr`
  owns the stage → commit → push → publish tail so the commit message is
  authored from the verified scope and the WIP is grouped into one coherent
  commit before push.
- **No squash, no history rewrite, no force-push.** Existing WIP commits are
  pushed as-is; the human chooses the merge strategy at merge time. A rejected
  push stops the skill — it is never forced past.
- **Confirm before publishing.** `git push` and `gh pr create` are the two
  outward-facing actions. Both run only after the user approves the drafted body
  (Phase 4), which also discloses any uncommitted files and the commit message.
- **Honest body + honest commit.** Do not list tests as run if they were not.
  The Test plan is a reviewer checklist. The commit message is the same title +
  Summary the user confirmed — never an invented or generic "WIP" message.
- **No sub-agents.** The skill runs synchronously and confirms directly with the
  user. Do not spawn agents.
- **Aborts are clean.** Every pre-flight failure prints one actionable hint and
  stops — no partial side effects, no commits or pushes left behind.
