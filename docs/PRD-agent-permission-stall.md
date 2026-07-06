# PRD — Agent Permission Stall & Transcript Tool-Use Rendering

## Status: 🔬 Investigated, decision deferred (2026-07-03)

This PRD captures a cluster of three issues found while running the
multi-project agent (see [`PRD-multi-project-claude-session.md`](./PRD-multi-project-claude-session.md)).
Two are bugs to fix; one is an open product/safety decision the user has
**deferred** pending further investigation. The investigation results are
recorded here so the decision can be made with full context.

---

## TL;DR

| # | Issue | Status | Owner |
|---|-------|--------|-------|
| 1 | **The stall** — Bash tool calls return `"This command requires approval"` and hang (no TTY) under the loopdeck-spawned `claude` process | Investigated; **fix deferred** (permission-mode decision) | decision: user |
| 2 | **"Didn't show up in loops.md"** — the orchestrator never writes its bookkeeping because it stalls (Issue 1) before reaching that step | **Symptom of #1** — resolves when #1 is fixed | — |
| 3 | **Persisted turns don't render tool calls / thinking** — `TurnBubble` shows `text` only; the streaming bubble's tool activity vanishes once the turn completes and reloads | Bug, **ready to fix** (no decision needed) | implementation |

---

## Evidence

### The stall (Issue 1)

Two transcripts of the *same* agent doing `git add`, captured from
`~/.claude/projects/.../<sessionId>.jsonl`:

**A — loopdeck-spawned process (`entrypoint: "sdk-cli"`), the broken one:**
```json
{"type":"assistant","message":{"content":[
  {"type":"tool_use","name":"Bash","input":{"command":"git add .loopdeck/"}}]}}
{"type":"user","message":{"content":[{"type":"tool_result",
  "content":"This command requires approval","is_error":true}]}}
```
The `tool_result` is an approval demand. The process has **no TTY** (piped
stdin/stdout), so the prompt can never be answered → the turn hangs until
`SEND_MESSAGE_TIMEOUT` (180s) in `claude_session.rs`.

**B — interactive CLI (`entrypoint: "cli"`), the working one:**
```json
{"type":"tool_result","content":"(Bash completed with no output)","is_error":false}
```
Same Bash command, runs fine. The only material difference is the entrypoint /
whether a TTY is present to satisfy the approval prompt.

### Why `acceptEdits` doesn't help

`claude_session.rs` spawns with `--permission-mode acceptEdits` (added in an
uncommitted working-tree change). The accompanying comment claims it "keeps Bash
guardrails intact while unblocking the autonomous loop." **That claim is wrong.**

Verified via `claude --help`:

```
--permission-mode <mode>   Permission mode to use for the session
                           (choices: "acceptEdits", "auto", "bypassPermissions",
                            "default", "dontAsk", "plan")
```

| Mode | What it auto-approves | Bash (`git add`, `cargo`, …) |
|------|----------------------|------------------------------|
| `default` | nothing — prompts for all | ❌ stalls (no TTY) |
| `acceptEdits` *(current)* | Edit / Write / NotebookEdit only | ❌ **stalls** ← the bug |
| `plan` / `safe` | read-only / restricted | ❌ can't act |
| `auto` | auto-approves within normal rule-set | ✅ mostly (policy denials can still stall) |
| `bypassPermissions` | skips all checks | ✅ always |

`acceptEdits` covers file *edits*, not command *execution*. The orchestrator's
loops are full of Bash (`git add`, `git commit`, `cargo test`, `npm run`), so
every loop stalls on the first command. **Evidence A above is this exact
failure.**

### "Didn't show up in loops.md" (Issue 2)

The `agent_start_loop` prompt instructs the agent:
> "…update `.loopdeck/loops.md` (mark the step `[x]`…)…"

But the agent does its file edits *first*, then runs `git add .loopdeck/`
(to stage its bookkeeping), and **stalls there** (Issue 1) before it can write
the loops.md update — or stalls immediately after writing, before the turn's
`result` event, so the turn is recorded as failed/timed-out. Either way the
loops.md bookkeeping never lands cleanly.

**This is a symptom, not a separate bug.** Fix Issue 1 and the orchestrator can
complete its write-through. (Separately, the loopdeck project's own
`.loopdeck/loops.md` Next Steps is stale — it's the pre-f189 backlog — but
that's data hygiene, not a defect.)

### Persisted turns drop tool calls (Issue 3)

`src/components/detail/Chat.tsx`:

- `StreamingBubble` (live, during a turn) renders tool calls via `ToolList`
  (line 291) and thinking via `ThinkingBlock` (line 288). ✅
- `TurnBubble` (persisted, shown after the turn completes and the transcript
  reloads) renders **only** `turn.text` + the meta row (lines 95–152). It never
  reads `turn.tool_calls` or `turn.thinking`, even though both fields are
  populated on the persisted `ConversationTurn` (`types/index.ts:166,168`) and
  written by the backend (`commands.rs` `send_and_record[_streaming]`).

**Observable effect:** during a turn you see the agent reading files / running
commands; the moment the turn finishes and the streaming bubble is replaced by
the persisted one, all that activity disappears. This is almost certainly the
"tool use didn't show up" report.

---

## Existing permission infrastructure (relevant to Issue 1's options)

The codebase already has machinery for writing Claude Code config into
bootstrapped projects — useful context for the option matrix below:

- `src-tauri/src/project.rs::bootstrap_project` → `skills::setup_hooks(repo_path)`
  writes a `.claude/settings.json` with a `PreToolUse` hook (matcher `Skill`)
  and a `Stop` hook. **It writes hooks, not a `permissions.allow` list, and the
  matcher is `Skill` only — not Bash.** So bootstrapped projects get *no* Bash
  auto-approval from this path.
- The loopdeck repo's own `.claude/settings.local.json` has a large
  `permissions.allow` list (`Bash(git add *)`, `Bash(cargo test *)`, …) — but
  that's *this* project's interactive-CLI allowlist, accumulated over time. It
  is **not** copied into spawned target projects, and the spawned `sdk-cli`
  process doesn't pick it up for other projects' working directories.

So today there is **no** Bash permission grant in effect for the spawned agent
running against e.g. `budget-manager-rust`. `acceptEdits` was the attempted
workaround, and it's insufficient.

---

## Issue 1 — Open decision (DEFERRED)

The user has deferred this until further investigation. The investigation is
done (above); what remains is a product/safety choice between four modes. All
four unblock the stall; they differ in **how much the autonomous agent can do to
the user's machine without asking.**

### Option A — `bypassPermissions` (full autonomy)

`claude_session.rs` spawns with `--permission-mode bypassPermissions`.

- **Pro:** Fully autonomous. `git add/commit`, `cargo`, `npm`, `rm` — anything
  the orchestrator needs, no stalls, ever. Matches `loopdeck-orchestrator`'s
  intent (drive itself through complete loops end-to-end). The agent already
  runs inside a trusted project with the user's git identity; a TTY-less spawn
  can't prompt safely anyway, so any non-bypass mode is partly theatre.
- **Con:** No guardrails. A misbehaving loop can `rm -rf` or force-push. Mitigated
  by: (a) the project's own git history is recoverable, (b) a future
  sandbox/container execution mode, (c) the per-project turn lock preventing
  concurrent mayhem.
- **Verdict:** simplest, highest capability, lowest friction.

### Option B — `auto` (guarded auto-approval)

Spawn with `--permission-mode auto`.

- **Pro:** Auto-approves within the normal rule-set, so most Bash runs. Keeps
  the default-deny posture for things that match a denial rule.
- **Con:** If a specific command still needs approval under `auto`, you're back
  to the **same stall** with no way to grant it (no TTY). Behaviour is also
  somewhat opaque — "auto" defers to policy we don't fully control.
- **Verdict:** middle path, but doesn't *guarantee* no stalls.

### Option C — `acceptEdits` + synthesized `permissions.allow` (precise)

Keep `acceptEdits` for edits, and have `skills::setup_hooks` (or a new helper)
**also** write a `.claude/settings.json` `permissions.allow` list into each
bootstrapped project, e.g. `Bash(git add:*)`, `Bash(git commit:*)`,
`Bash(cargo:*)`, `Bash(npm:*)`, `Bash(git status:*)`.

- **Pro:** Most precise — agent can edit freely and run a *known* set of
  commands; anything novel still prompts (and thus stalls, visibly, rather than
  running silently). Aligns with the existing `setup_hooks` per-project config
  pattern. The allowlist is project-visible/auditable.
- **Con:** You maintain the list. A novel-but-legitimate command stalls until
  added. Two-source-of-truth risk (the Rust allowlist vs. reality).
- **Verdict:** safest-by-design, highest setup cost, still stalls on the
  unexpected.

### Option D — Hybrid: `bypassPermissions` + belt-and-suspenders

`bypassPermissions` at spawn, **plus** (1) a clear warning in the Agent tab
when a turn is running, and (2) a future hook/sandbox. Defers the hard part
(sandboxing) to a later PRD while unblocking everything now.

- **Pro:** Unblocks today; honest about the tradeoff; path to real safety later.
- **Con:** Until the sandbox lands, the agent is unrestricted during a turn.

### Recommendation (to revisit at decision time)

**Option A (`bypassPermissions`)** for v1, with a follow-up PRD for a sandboxed
execution mode (Option D's future hook). Reasoning: the spawned agent is
inherently non-interactive, so any mode that can prompt is a latent stall; the
`acceptEdits` experiment proved that "partial" permission modes just move the
stall. But this is the user's call to make.

### What still needs verifying before deciding

- [ ] Does `--permission-mode auto` actually run `git push` / `rm` without
      prompting, or does it still gate destructive commands? (Needs a live test
      against the provider — the `--help` text is ambiguous.)
- [ ] Does the `permissions.allow` syntax in `settings.json` support the broad
      patterns needed (`Bash(git:*)` vs `Bash(git add:*)`)? Confirm against a
      real bootstrapped project.
- [ ] Whether `bypassPermissions` interacts badly with the existing
      `PreToolUse:Bash` / `rtk-rewrite.sh` hook (the logs show rtk firing; under
      bypass it still runs but can't block — acceptable, just confirm).

---

## Issue 3 — Fix (no decision needed)

Make `TurnBubble` render `tool_calls` and `thinking` the same way
`StreamingBubble` does, so a completed turn retains its activity trail instead
of collapsing to bare text.

### Scope
- `src/components/detail/Chat.tsx` — `TurnBubble`:
  - Render `<ThinkingBlock thinking={turn.thinking ?? ""} />` before the text
    body (assistant turns only), matching `StreamingBubble`'s placement.
  - Render `<ToolList tools={turn.tool_calls ?? []} />` for assistant turns.
    Unlike the streaming variant (which hides tools once complete), the
    persisted bubble should show them **always** — they're history, not live
    activity — so no `!isComplete` gate.
- No backend change: `tool_calls` / `thinking` are already persisted.
- No type change: both fields already exist on `ConversationTurn`.

### Verification
- `npx tsc --noEmit` clean.
- Manual: run a turn that makes tool calls; after completion + reload, the
  tool calls and thinking remain visible (previously they vanished).
- Regression: user turns and text-only assistant turns render unchanged.

### Out of scope
- Collapsing long tool-call lists (future — a transcript with 50 tool calls
  could get noisy; consider a "show N tool calls" disclosure later).
- Rendering tool *results* (currently neither bubble does; separate enhancement).

---

## Implementation order (when the decision is made)

1. **Issue 3** — ship now (independent of the Issue 1 decision).
2. **Issue 1** — once the mode is chosen, a one-line change in
   `claude_session.rs` (`cmd.args(["--permission-mode", "<mode>"])`), plus for
   Option C a new helper in `skills.rs` called from `bootstrap_project`.
3. **Issue 2** — no code; verify it resolves once #1 lands (the orchestrator
   reaches its loops.md write-through).

## Files touched (planned)
- `src/components/detail/Chat.tsx` (Issue 3 — ready)
- `src-tauri/src/claude_session.rs` (Issue 1 — deferred decision)
- `src-tauri/src/skills.rs` + `src-tauri/src/project.rs` (Issue 1, Option C only)
