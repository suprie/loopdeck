---
name: loopdeck:orchestrator
description: Orchestrate feature implementation from a PRD. Use when the user asks to "build from PRD", "implement the spec", "orchestrate this feature", "execute the plan", or points to a PRD/spec file and wants it implemented. Reads the PRD, asks clarifying questions, invokes API expert for contract, spawns parallel Go+iOS agents, runs code review, stitches everything together, and decides next steps.
argument-hint: <prd-file-path>
allowed-tools: [Read, Write, Edit, Glob, Grep, Bash, Agent, TaskCreate, TaskUpdate, Skill]
---

# Orchestrator — PRD → Clarify → API Contract → Build → Review → Stitch → Verify → Ship

Read a Product Requirements Document (PRD), ask clarifying questions, produce an API contract, spawn parallel backend (Go) and frontend (iOS) agents, review output, stitch everything together, verify against the PRD's acceptance criteria, and ship a reviewable pull request.

## Full Orchestration Flow

```
┌──────────────────────────────────────────────────────┐
│  1. Read PRD & Ask Clarifying Questions               │
│     Parse the PRD, identify ambiguities, ask user     │
├──────────────────────────────────────────────────────┤
│  2. Phase Decomposition + API Contract                │
│     Plan phases. Invoke api-expert to create the API  │
│     contract (OpenAPI spec, endpoints, schemas).      │
├──────────────────────────────────────────────────────┤
│  3. Parallel Build — Go (Backend) + iOS (Frontend)    │
│     Spawn agents for both stacks simultaneously,      │
│     each driven by the shared API contract.           │
│     ├─ Go agents: go-dev skill, implement server      │
│     └─ iOS agents: ios-dev skill, implement client    │
├──────────────────────────────────────────────────────┤
│  4. Code Review (per stack)                           │
│     ├─ go-code-review on backend output               │
│     └─ ios-code-review on frontend output             │
├──────────────────────────────────────────────────────┤
│  5. Stitch & Integration Check                        │
│     Verify Go ↔ iOS contract alignment, wire          │
│     everything together, run tests.                   │
├──────────────────────────────────────────────────────┤
│  6. Verify Against PRD                                │
│     Invoke prd-verifier → PASS / WARN / BLOCK;        │
│     BLOCK / WARN gate the ship step.                  │
├──────────────────────────────────────────────────────┤
│  7. Decide & Open PR                                  │
│     On a green verdict, invoke open-pr to ship a      │
│     reviewable PR.                                    │
└──────────────────────────────────────────────────────┘
```

## Worktree Discipline (every loop is isolated)

**Every orchestration loop runs inside its own git worktree — never in the main working tree.** This is the user's standing instruction.

- **Start of the loop (before Phase 1):** call `EnterWorktree` with a name for the feature, e.g. `EnterWorktree({ name: "feat/<feature>" })`. This satisfies the tool's "only when explicitly instructed" gate. The entire PRD → clarify → build → review → stitch flow happens inside this worktree.
- **Parallel agents (Phase 3+):** spawn them *after* entering the worktree so they inherit its working directory. By the rules below they work on non-overlapping files, so they can safely share the loop's worktree. Only when two agents must edit the *same* file: serialize them, or give the second one its own `isolation: "worktree"` and merge its branch back during the Stitch phase.
- **End of the loop (Phase 7):** call `ExitWorktree({ action: "keep" })`. Do **not** auto-remove — the user wants worktrees retained on their branch for review/merge.

## Phase 1: Read PRD & Ask Clarifying Questions

### Step A: Parse the PRD

1. Read the PRD file the user provided via `$ARGUMENTS`
2. Extract and summarize:
   - **Feature name** and **goal**
   - **User stories** or use cases
   - **Screens / UI components** required (iOS frontend)
   - **Data models** and entities mentioned
   - **Business rules** and constraints
   - **Acceptance criteria**
   - **Edge cases** and error states
   - **External dependencies** (third-party APIs, auth provider, payment, push notifications)

### Step B: Generate Clarifying Questions

Before any planning, identify ambiguities in the PRD. Group questions by topic:

```markdown
## Clarifying Questions — [Feature Name]

### Auth & Security
1. Is authentication session-based (cookie) or token-based (JWT)?
2. Which fields are required vs optional for user registration?
3. ...

### Data Model
1. Can a `CoffeeShop` have multiple locations, or is it 1:1?
2. What's the max length for the `description` field?
3. ...

### API Behavior
1. Should the shop list endpoint support pagination? Cursor or offset?
2. What's the expected sort order for search results?
3. ...

### UI / UX
1. Should the login screen support biometric auth (Face ID / Touch ID)?
2. What's the loading state design — skeleton or spinner?
3. ...

### Error Handling
1. What error message should be shown when the user is offline?
2. Should we retry failed requests automatically?
3. ...

### Non-Functional
1. Target iOS version?
2. Expected response time for the shop list endpoint?
3. ...
```

**Present questions to the user and wait for answers.** Do not proceed until the user responds. The user may answer all, some, or defer questions to later.

## Phase 2: Phase Decomposition & API Contract

### Step A: Create the Phase Plan

Based on the PRD and clarified answers, produce a multi-stack phase plan:

```markdown
## Implementation Plan — [Feature Name]

### Phase 0: API Contract (api-expert)
| Task | Skill | Agent |
|------|-------|-------|
| OpenAPI spec (all endpoints, schemas, errors) | api-expert | 1 |

### Phase 1: Shared Foundation
| Stack | Tasks | Agents |
|-------|-------|--------|
| Go | Project scaffold, module init, folder structure | 1 |
| iOS | Domain models, protocol definitions | 1 |

### Phase 2: Core Services
| Stack | Tasks | Agents |
|-------|-------|--------|
| Go | Database schema, migrations, repository layer | 2 |
| iOS | Adapters (network service, auth storage, keychain) | 2 |

### Phase 3: Business Logic
| Stack | Tasks | Agents |
|-------|-------|--------|
| Go | Handlers, use cases, middleware | 3 |
| iOS | Interactors (auth, shop list, shop detail) | 3 |

### Phase 4: Presentation
| Stack | Tasks | Agents |
|-------|-------|--------|
| Go | — (API-only, no UI) | 0 |
| iOS | ViewModels + SwiftUI Views | 4 |

### Phase 5: Tests
| Stack | Tasks | Agents |
|-------|-------|--------|
| Go | Unit tests, integration tests | 2 |
| iOS | ViewModel tests, Interactor tests | 3 |

### Phase 6: Verify Against PRD
| Task | Agent |
|------|-------|
| DI wiring, navigation, contract alignment check | 1 |
| Full test suite run | 1 |
| Per-criterion PASS / PARTIAL / FAIL via `prd-verifier` | 1 |

### Phase 7: Decide & Open PR
| Task | Skill |
|------|-------|
| On a green verdict, `open-pr` drafts the PR body and runs `gh pr create` | 1 |
```

**Wait for user approval** before proceeding.

### Step B: Invoke API Expert

Before any implementation, invoke the `api-expert` skill (or spawn an agent with api-expert context) to create the API contract:

1. Provide the PRD summary + clarified answers as input
2. The API expert produces:
   - An OpenAPI 3.0 spec file at `api/openapi.yaml`
   - API documentation at `docs/api/README.md`
   - Request/response schemas for every endpoint
   - Error response formats
   - Examples for key endpoints
3. **Review the contract with the user.** This is the source of truth both stacks will build against.

The API contract must be locked before Phase 1 begins. Changes to the contract later mean rework in both stacks.

## Phase 3: Parallel Build — Go + iOS

### Step A: Spawn Stack Agents in Parallel

Both stacks build simultaneously against the same API contract. Within each stack, tasks that don't share files run in parallel.

Each agent prompt must include:
- The API contract file path (`api/openapi.yaml`) — every agent reads this
- The specific files to create/modify
- PRD context (user stories, business rules, edge cases)
- The relevant skill conventions (`go-dev` or `ios-dev`)
- File paths of completed prior-phase output for that stack

**Go agent prompt template:**

```
Create the [component] in the Go backend.

API contract: api/openapi.yaml — read endpoints [GET/POST /xxx] and their schemas.

Implement:
- [specific file paths and what goes in each]
- Follow go-dev skill conventions (Clean Architecture, handler → usecase → repository)
- Use the shared models from [prior phase output paths]
- Write unit tests for the business logic layer

PRD context: [relevant rules, edge cases, acceptance criteria]
```

**iOS agent prompt template:**

```
Create the [component] in the iOS app.

API contract: api/openapi.yaml — read endpoints [GET/POST /xxx] and their schemas.

Implement:
- [specific file paths and what goes in each]
- Follow ios-dev skill conventions (MVVM + Interactor + Adapter, DI, protocols)
- Use the shared models from [prior phase output paths]
- Write unit tests for ViewModel and Interactor

PRD context: [relevant rules, edge cases, acceptance criteria]
```

### Step B: Wait for All Agents

All agents within a phase (across both stacks) run in parallel. Wait for completion before moving to review.

## Phase 4: Code Review (Per Stack)

After each phase, run stack-specific reviews:

### Go Review

Invoke `go-code-review` on all Go files produced in this phase. Check:
- Clean Architecture layers (handler → usecase → repository)
- Error handling (no ignored errors, wrapped context)
- Interface segregation
- Test coverage

### iOS Review

Invoke `ios-code-review` on all iOS files produced in this phase. Check:
- MVVM + Interactor + Adapter layering
- Protocol-driven DI
- ViewModel isolation
- Test coverage

### Review Verdict & Actions

| Verdict | Action |
|---------|--------|
| ✅ Both pass | Proceed to next phase |
| ⚠️ Warnings only | Proceed, create follow-up task for warnings |
| ❌ Blocker in one stack | Pause that stack, continue the other if possible. Offer to rework. |
| ❌ Blockers in both | Stop. Report to user. Offer targeted fix agents. |

## Phase 5: Stitch & Integration Check

After all build phases complete, run the integration phase:

### Contract Alignment Check

Verify the Go server and iOS client match the API contract:

1. Compare Go handler signatures against `api/openapi.yaml` endpoints
2. Compare iOS adapter request/response models against the same schemas
3. Flag any field name mismatch, missing endpoint, or type difference

### Wiring Check

- **Go**: Verify `main.go` / router registers all handlers, middleware chain is correct
- **iOS**: Verify DI container / composition root wires all ViewModels → Interactors → Adapters
- **Shared**: Check that base URL, auth header name, error codes are consistent

### Run Tests

```bash
# Go
cd server && go test ./... -v -cover

# iOS
xcodebuild test -scheme NgopiYuk -destination 'platform=iOS Simulator,name=iPhone 16'
```

### Final Review

Run a cross-stack review:
- `go-code-review` — full server
- `ios-code-review` — full client
- Manual check: can the iOS app reasonably call the Go server as specified?

## Phase 6: Verify Against PRD

Before opening a PR, verify the implemented code against the PRD's **stated
acceptance criteria**. This is a structured per-criterion pass/fail — distinct
from Phase 4's code-quality reviews and Phase 5's contract-alignment check,
neither of which assesses whether the PRD's acceptance criteria are actually met.

### Invoke the verifier

Call the `loopdeck:prd-verifier` skill with the PRD path from `$ARGUMENTS`. The
skill is read-only (`allowed-tools: [Read, Glob, Grep, Bash]` — no
`Edit`/`Write`/`Agent`); it produces a per-criterion **PASS / PARTIAL / FAIL**
table with `file:line` evidence, a non-goals scope-creep audit, and a single
greppable roll-up verdict:

- any **FAIL** → **BLOCK**
- else any **PARTIAL** → **WARN**
- else → **PASS**

### Verdict & Actions

| Verdict | Action |
|---------|--------|
| **PASS** (all criteria green) | Proceed to Phase 7 — decide & open PR. |
| **WARN** (one or more PARTIAL) | Spawn targeted fix agents for the PARTIAL criteria, then re-verify. Do not proceed to Phase 7 until WARN clears or the user explicitly accepts the partial. |
| **BLOCK** (one or more FAIL) | Return to Phase 3 for the failing scope; re-spawn agents with corrected prompts. Do not ship. |
| Non-goals scope creep flagged | Surface to the user; let the user decide whether to retract the scope or amend the PRD. The audit is a flag, not a FAIL. |

Re-verify on every rework loop — the verdict is what tells the orchestrator to
proceed vs. rework, so verifying only at the very end defeats the gate.

## Phase 7: Decide & Open PR

Based on the Phase 6 verify verdict and the Phase 5 integration results:

| Verdict / Outcome | Action |
|---------|--------|
| Verify **PASS** + integration green | Report completion (files created, test coverage, deferred warnings). **Invoke `loopdeck:open-pr`** to ship the verified work as a reviewable PR. |
| Verify **WARN** (one or more PARTIAL) | Spawn targeted fix agents for the PARTIAL criteria, re-verify. Do **not** ship until WARN clears or the user explicitly accepts the partial. |
| Verify **BLOCK** (one or more FAIL) | Return to Phase 3 for the failing scope; re-spawn agents with corrected prompts. Do **not** ship. |
| Integration issues (minor) | Spawn targeted fix agents, re-run affected checks. |
| Major gaps | Return to the relevant phase, re-spawn agents with corrected prompts. |
| PRD gap discovered | Flag to user — the PRD may need an amendment. Do not guess. |
| Non-goals scope creep flagged | Surface to the user; let the user retract the scope or amend the PRD. |

### Open PR (green verdict only)

When the Phase 6 verdict is **PASS** and Phase 5's integration check is green:

1. Call `loopdeck:open-pr`. The skill runs pre-flight checks, gathers
   `.loopdeck/` context, drafts a PR body (Summary / What changed / PRD /
   Decisions / Test plan), and shows it to the user for confirmation.
2. After the user confirms the body, `open-pr` commits any uncommitted work
   (message authored from the verified scope), pushes, and runs
   `gh pr create --web`. Report the returned PR URL.
3. `open-pr` records the PR URL in `.loopdeck/loops.md ## Next Steps` itself.

Do **not** call `open-pr` on a WARN or BLOCK verdict. Ship only green work. The
verify verdict is the gate: `open-pr` is independently invocable, but the
orchestrator only reaches it through a PASS.

If the feature spans multiple PRDs or epics, return to Phase 1 with the next PRD
after this PRD's work is shipped.

# LoopDeck Memory Convention

This project uses `.loopdeck/` for persistent project memory. The standard AI workflow writes to these files so LoopDeck's UI displays them.

## Files

| File | Purpose | When to Write |
|------|---------|---------------|
| `.loopdeck/current-loop.md` | Active loop snapshot (created by hook on orchestrator start) | Auto-created by `orchestrator-start` PreToolUse hook |
| `.loopdeck/decisions.md` | Lightweight ADRs (architectural decision records) | After any significant design/architecture decision |
| `.loopdeck/loops.md` | Current loop status, next steps, history | At the end of every session |

## Auto-Write Convention

### decisions.md Format

Write decisions as level-2 headings with date and title, followed by key-value bullets and body text:

```markdown
# Decisions

## YYYY-MM-DD — Title of the decision
- **Status**: proposed | accepted | superseded
- **Context**: Why this decision was needed.
- **Consequences**: What follows from this decision.

Additional body text explaining the decision in more detail.
```

**Rules:**
- Use `## YYYY-MM-DD — Title` format (em dash preferred, hyphen accepted)
- `Status` must be one of: `proposed`, `accepted`, `superseded`
- `Context` explains the situation that prompted the decision
- `Consequences` captures what changed because of this decision
- Body text after the bullets adds detail
- **Append** new decisions — never delete old ones
- **Append to the file after each phase** — do this as part of the Phase 7 "Decide & Open PR" step

### current-loop.md Format

A single line of plain text — the high-level summary of the active loop. Displayed on the LoopDeck dashboard project card.

```markdown
UI restyling — Tailwind CSS v4, OKLCH dark palette, sidebar layout
```

**Rules:**
- **Max 100 characters** — this is a dashboard card label, not a detailed description
- **Single line** — no markdown bullets, no headings, no newlines
- **High-level summary only** — what is being worked on right now, in one sentence
- Keep details (start date, status, next steps) in `loops.md`

### loops.md Format

Write loops as level-2 sections for Current/Next Steps/History, with level-3 entries for historical loops:

```markdown
# Loops

## Current
- **Started**: YYYY-MM-DD
- **Goal**: What this loop aims to accomplish
- **Status**: in_progress

## Next Steps
- [ ] Task one
- [ ] Task two

## History

### YYYY-MM-DD — Completed loop title
- **Status**: completed
- **Completed**: YYYY-MM-DD
```

When a loop was promoted from an epic/PRD (via the LoopDeck UI), the
`## Current` block carries back-reference bullets that trace it back to the
spec layer:

```markdown
## Current
- **Started**: YYYY-MM-DD
- **Goal**: Define Epic and Prd structs in epic.rs
- **Status**: in_progress
- **Epic**: support-project-management
- **PRD**: prd-spec-layer
```

Treat `**Epic**` and `**PRD**` as read-only context — they tell you *why* the
loop exists. Never edit, remove, or reorder them. The `**Goal**` is the only
field that drives your work.

**Rules:**
- `## Current` contains the active loop (or `_No active loop._` if none)
- `## Next Steps` is a checklist of `- [ ]` items for the current loop
- `## History` contains completed/abandoned loops as `### YYYY-MM-DD — Title` entries
- At the end of every session, update the Current loop status and Next Steps
- When a loop completes, move it to History and start a new Current loop
- **When moving a loop to History, if its `## Current` block carried `**Epic**`
  and `**PRD**` back-references, check the matching `- [ ]` box in the origin
  PRD file.** The PRD lives at
  `docs/epics/<Epic-slug>/<PRD>.md`, under a `## Phases` → `### Phase N`
  checklist. Find the item whose text matches the loop's `**Goal**` and change
  `- [ ]` to `- [x]`. This keeps the spec layer in sync with what's actually
  been built. If the item isn't found (title drifted, or the file was
  removed), skip silently — the human can check it manually in the UI.

## Phase Actions

After each phase completes:
1. If you made an architectural decision → append to `.loopdeck/decisions.md`
2. At the end of each phase → update `.loopdeck/loops.md` Next Steps

At the end of the session (Phase 7 or equivalent):
1. Update `.loopdeck/loops.md` Current status and Next Steps
2. Append any unrecorded decisions to `.loopdeck/decisions.md`
3. If a loop completed, move it to History and set the next Current loop
4. **If the completed loop carried `**Epic**`/`**PRD**` back-references, check
   the matching `- [ ]` box in the origin PRD** at
   `docs/epics/<Epic>/<PRD>.md` (find the item matching the loop's `**Goal**`,
   change `- [ ]` to `- [x]`). Skip silently if not found.

## Integration with Phase 7

In the final "Decide & Open PR" step, after reporting results:
- Write/update the files as described above
- Ensure the next loop's goal is recorded in loops.md so the next session picks it up

## Important Rules

- **One worktree per loop.** Every orchestration loop starts with `EnterWorktree` (named for the feature) and ends with `ExitWorktree({ action: "keep" })`. The whole flow runs inside it; never edit the main tree directly. See Worktree Discipline above.
- **Clarify first.** Never assume. If the PRD is ambiguous, ask before building.
- **API contract is law.** Both stacks build against it. Contract changes cascade to both stacks — flag them explicitly.
- **Both stacks in parallel.** Go and iOS build simultaneously within each phase. Don't serialize unless one depends on the other's output (rare).
- **Review every phase.** No unreviewed code proceeds to the next phase.
- **User approves the plan.** Present the phase decomposition. Don't spawn agents until the user says go.
- **Keep agents focused.** One agent = one cohesive unit of work (a single file or tightly-related small group).
- **Preserve prior output.** Later-phase agents must read (not recreate) files from earlier phases. Include file paths in prompts.
