---
title: Agent Full Access
slug: agent-full-access
milestone: "0.3.0"
status: completed
started: 2026-07-21
owner: Suprie
description: >
  Add a per-project "full access" permission tier for LoopDeck-spawned agents
  (auto-approve control requests while keeping the destructive-command floor
  and audit trail), then close the orchestrator loop with two focused skills:
  PRD-acceptance verification and gh-based pull request creation. Delivers the
  "the agent runs autonomously, verifies itself against the spec, and ships"
  workflow without giving up the safety floor that distinguishes LoopDeck from
  a raw bypassPermissions spawn.
---

# Epic — Agent Full Access + Verify + Ship

## Motivation

LoopDeck today runs every agent under `ConfirmChanges` — mutating/executing
tool calls park on a manual-approval card. That posture is correct for a
brand-new install and for untrusted repos, but it stalls the agent on every
`git add`, `cargo build`, and `npm test` once the user has decided they trust
the project. The current workaround is `.claude/settings.local.json` allow
rules, which the user must hand-curate per project and which the
trust-boundary PRD explicitly forbids as a hidden default.

Meanwhile the orchestrator skill ends at "Decide Next Phase": it stages files
for commit (`git add`) but never verifies that the implemented code actually
satisfies the PRD's acceptance criteria, and it never opens a pull request.
The loop is incomplete — the user still has to eyeball the diff against the
spec and run `gh pr create` by hand. That's the gap between "the agent built
something" and "the agent shipped something reviewable."

This epic closes both gaps together, deliberately:

- **Full access** removes the per-call friction so an agent can run a whole
  feature without seven approval prompts.
- **PRD verification** makes that autonomy accountable — before the agent is
  allowed to ship, it must show its work against the spec.
- **Open PR** turns the verified work into a reviewable artifact, carrying the
  PRD link and LoopDeck memory into the PR body.

Shipping all three in one milestone is what makes full access safe enough to
ship: autonomy without verification is reckless, verification without a ship
step leaves the work stranded on a branch.

## Scope

In scope:

- A new `FullAccess` variant on the Rust `PermissionMode` enum
  (`permission.rs`), implemented as **policy auto-allow** — every
  control_request that clears the destructive floor is answered `allow`, but
  the floor (rm -rf, force-push, pipe-to-shell, …) still hard-denies and every
  decision still logs. This is explicitly NOT `claude --permission-mode
  bypassPermissions` — LoopDeck keeps the audit trail and the safety net.
- A persisted per-project `permission_mode` field on `ProjectEntry`
  (`config.rs`), with serde `default` for backward compatibility. Opt-in per
  project, not global.
- `set_project_permission_mode` IPC command (mirrors `set_agent_config`).
- Frontend: make `PermissionModeBadge` mode-aware and add a per-project tier
  selector in the Agent panel toolbar (`AgentPanel.tsx`, `AgentRunner.tsx`).
- A new `loopdeck:prd-verifier` skill (`.agents/skills/loopdeck-prd-verifier/`)
  — read-only, verifies implemented code against a PRD's acceptance criteria,
  returns a pass/fail report with `file:line` evidence per criterion.
- A new `loopdeck:open-pr` skill (`.agents/skills/loopdeck-open-pr/`) —
  pre-flight checks, generates a PR body from `.loopdeck/` memory, runs
  `gh pr create --web` after user confirmation.
- Orchestrator wiring: a new "Verify Against PRD" phase (Phase 6) that invokes
  the verifier, and a "Decide & Open PR" step at the end of the final phase
  that invokes `open-pr` when the verdict is green.

Out of scope (deferred to later milestones or parked):

- OS sandboxing (Non-Goal of `PRD-trust-boundary-hardening.md`).
- The `claude --permission-mode bypassPermissions` CLI flag flip — the
  "dangerous bypass" tier with no floor. (See ADR-1; revisit if a credible
  sandbox PRD lands.)
- Per-spawn tier selection (only per-project). Per-spawn would require new
  parameters on the streaming IPC signatures; per-project avoids that.
- Mirroring the new skills to `.claude/skills/` — that directory is
  gitignored and is the Claude Code client's own copy. ZCode reads
  `.agents/skills/`; that is the canonical path.
- AI-generated PRDs and AI phase decomposition → 0.4.0 (per the
  `support-project-management` epic).
- Runtime skill injection (the Parking Lot "Move agent control into LoopDeck
  app" item in `loops.md`) — this epic ships skills as static files; the
  app-owned spawn-time injection is a later milestone.

## Non-Goals

- **Auto-merging or auto-deploying PRs.** `open-pr` runs `gh pr create`; the
  human reviews and merges. No `gh pr merge`, no auto-rebase.
- **Verifying non-functional requirements** (performance, load, a11y). The
  verifier checks the PRD's stated acceptance criteria; NFRs are out of scope
  unless the PRD lists them as criteria.
- **Enforcing that `open-pr` only runs after `prd-verifier` passes.** The two
  skills are independently invocable. The orchestrator wires them in sequence;
  a human calling `open-pr` directly is trusted to have verified already.
- **Updating `PRD-trust-boundary-hardening.md` to formally document a third
  mode.** The `FullAccess` tier is deliberately scoped to this epic and the
  `agent-full-access-tier` PRD; the trust-boundary PRD continues to describe
  the two-mode contract. A cross-reference is added, not a rewrite.

## PRD Index

| PRD | Covers |
|-----|--------|
| [prd-full-access-tier.md](./prd-full-access-tier.md) | `FullAccess` permission mode (Rust), persisted per-project field, `set_project_permission_mode` IPC, frontend tier selector, mode-aware badge |
| [prd-verify-and-ship-skills.md](./prd-verify-and-ship-skills.md) | `loopdeck:prd-verifier` + `loopdeck:open-pr` skills, orchestrator Phase 6 verify + final ship step |

## Architecture Decisions

### ADR-1: Policy auto-allow, NOT CLI `bypassPermissions`

**Context.** "Full access" could mean two very different things at the Claude
CLI spawn site (`claude_session.rs:358`):

1. **CLI bypass.** Flip `--permission-mode default` to `bypassPermissions`.
   Claude itself auto-approves every tool call; LoopDeck's
   `MANUAL_APPROVAL_TOOLS` interception, destructive floor, and audit trail
   are all bypassed.
2. **Policy auto-allow.** Keep `--permission-mode default` so Claude still
   routes every tool call over stdio as a `control_request`; LoopDeck's policy
   answers `allow` to every request that clears the destructive floor.

The historical PRD `PRD-agent-permission-stall.md` weighed these as Options A
and D and flagged CLI bypass as "no guardrails, needs a follow-up sandbox
PRD." No such sandbox PRD has landed.

**Decision.** Policy auto-allow. The `FullAccess` variant lives on the Rust
`PermissionMode` enum; `decide()` returns `Allow` after the floor check. The
CLI flag stays `default`.

**Consequences.** The destructive floor at `permission.rs:180-493` still
hard-denies `rm -rf /`, `git push --force`, pipe-to-shell, etc., even under
full access — a mis-configured agent cannot trash the user's system. Every
decision still flows through the audit path. The cost is one extra stdio round
trip per tool call (negligible vs. the LLM latency). A future "dangerous
bypass" tier, if ever needed, is a separate variant that flips the CLI flag
and is gated behind its own PRD.

### ADR-2: Per-project persistence, not per-spawn

**Context.** The tier could be persisted per-project (on `ProjectEntry` in the
global registry) or passed per-spawn (a new parameter on
`agent_send_message_streaming` / `agent_start_loop_streaming`).

**Decision.** Per-project. The field lives on `ProjectEntry::permission_mode`
with serde `default = ConfirmChanges`, so existing `config.yaml` entries
deserialize unchanged. The streaming IPC signatures are untouched.

**Consequences.** Changing the tier requires a UI action, not a prompt prefix,
and takes effect on the next spawn (documented in the selector popover). This
matches the trust-boundary PRD's framing of "Autonomous project" as a
per-project opt-in. The runtime policy is read once in `spawn_fresh`
(`commands/agent.rs`) via a new `project_permission_policy(state, path)`
helper, defaulting to `confirm_changes()` for unregistered paths.

### ADR-3: `FullAccess` is a new variant, not an alias for `AutonomousProject`

**Context.** `permission.rs` documents a deferred `AutonomousProject` variant
gated on "Phase 3 path-containment helpers." The trust-boundary PRD defines
"Autonomous project" as auto-allowing file mutation *inside the project root*
while still gating Bash/MCP/out-of-root.

**Decision.** `FullAccess` is a distinct variant with broader semantics:
auto-allow everything that clears the floor, no path containment, no MCP
gating. `AutonomousProject` remains deferred for when path-containment lands.

**Consequences.** Two permissive tiers eventually coexist: `AutonomousProject`
(safe-bounded, project-root-scoped) and `FullAccess` (unbounded, opt-in per
project, floor-only). The UI exposes only `FullAccess` in this epic. The
naming makes the risk profile visible at the type level — `FullAccess` reads
as "no extra gating," `AutonomousProject` reads as "bounded."

### ADR-4: Verify is a read-only skill, not a hook

**Context.** PRD verification could be (a) a standalone skill invoked
on-demand, (b) an orchestrator phase only, or (c) a Stop hook that blocks
every session end until acceptance criteria pass.

**Decision.** Standalone skill (`loopdeck:prd-verifier`), wired into the
orchestrator as a phase. Read-only (`allowed-tools: [Read, Glob, Grep, Bash]`
— no `Edit`/`Write`). Never a hook.

**Consequences.** Verification is reusable outside the orchestrator (a human
can run `/loopdeck:prd-verifier docs/epics/foo/prd-bar.md` anytime) and never
blocks the agent unexpectedly. The orchestrator's Phase 6 invokes it
explicitly. This mirrors ADR-1 of `support-project-management`: the skill is
mechanics, the orchestration is a separate concern.

### ADR-5: `open-pr` confirms with the user before `gh pr create`

**Context.** A skill that pushes commits and opens a PR without confirmation
is outward-facing and hard to reverse.

**Decision.** `loopdeck:open-pr` gathers context, generates the PR body,
**shows the body to the user for confirmation**, then runs
`gh pr create --web`. The pre-flight checks (`gh auth status`, branch ≠ main,
upstream pushed) abort cleanly with a remediation hint on failure.

**Consequences.** The orchestrator's final ship step is a two-step
interaction (draft → confirm), not a fire-and-forget. This respects the
"outward-facing actions require confirmation" rule for actions that publish
to a remote. The user always sees the PR body before it ships.

## Success Criteria

- A user can flip a registered project to "Full access" in the Agent panel
  UI; the next agent spawn runs Bash/Edit/Write calls without approval cards.
- Under "Full access," a `rm -rf /`-shaped Bash call still hard-denies with
  the destructive-floor reason, and the decision is visible in the audit log.
- An existing `config.yaml` with no `permission_mode` field deserializes
  cleanly and defaults to `ConfirmChanges`.
- Invoking `/loopdeck:prd-verifier <prd-path>` returns a per-criterion
  pass/fail table with `file:line` evidence, never edits files.
- Invoking `/loopdeck:open-pr` from a feature branch produces a PR whose body
  links the PRD and summarizes `.loopdeck/` memory, after explicit user
  confirmation of the body.
- Running the orchestrator end-to-end on a small PRD produces, in order: a
  verify report, then a draft PR body, then a created PR URL.
- The skills operate **stack-agnostically** on any project LoopDeck has
  imported — Go, Android (Gradle), PHP (Composer), iOS (Xcode/Swift),
  Ruby, Python, Node, Rust, etc. The `open-pr` "Test plan" section is
  inferred from detected markers, never hardcoded to one stack.
- LoopDeck's own implementation of the tier (the Rust + TypeScript changes
  in `prd-full-access-tier.md`) passes `cargo test`,
  `cargo clippy -D warnings`, and `npx tsc --noEmit`. (This criterion scopes
  to LoopDeck itself, which is Rust + Tauri + React — not to managed
  projects.)

## Risks

| Risk | Mitigation |
|------|-----------|
| Users enable `FullAccess` on an untrusted repo and the agent damages files inside the project root | The destructive floor still applies; project-root damage is bounded to the project (which is already under git). Document in the selector popover: "applies to the next conversation; the destructive floor still applies." |
| PRD verifier produces false-positive PASS verdicts (claims criteria are met when they aren't) | The skill must cite `file:line` evidence per criterion and quote code briefly. The report is a review aid, not a gate — the human still reviews the PR. The orchestrator treats PARTIAL as a rework signal, not a silent pass. |
| `open-pr` pushes to the wrong branch or opens a PR against the wrong base | Pre-flight checks abort if branch is `main`/`master` or if upstream is not pushed. Base branch defaults to the repo's default; user confirms the body before `gh pr create` runs. |
| Policy auto-allow adds latency that makes the agent feel slower than CLI bypass | The extra cost is one stdio round trip per tool call (sub-millisecond locally), negligible vs. LLM latency. If it proves material, ADR-1 can be revisited without changing the data model. |
| `loopdeck-` prefixed skills get clobbered by a future `copy_skills` version bump | Acknowledged in `decisions.md:153` (managed-skills refresh). The skills are version-stamped via the manifest; user customizations belong in a non-prefixed copy. |
| The `FullAccess` tier drifts from the trust-boundary PRD's two-mode contract | This epic ships a cross-reference, not a rewrite. A follow-up PRD can fold `FullAccess` into the trust-boundary doc once the runtime semantics are proven in the alpha. |
