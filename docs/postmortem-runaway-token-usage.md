# Postmortem — Runaway Token Usage (142M tokens / day)

## Status: ✅ Partially resolved (2026-07-19)

A user reported spending ~30 million tokens in under an hour, then later ~142
million in a day, while driving LoopDeck development. The bill was real. The
first investigation misdiagnosed the cause. The second investigation corrected
it. This document captures both rounds — including why the first was wrong —
so the same diagnostic discipline gaps don't recur.

**The short version:** 94% of the consumed tokens were **cache reads** — the
same growing conversation context being replayed on every turn. The dominant
lever is **session length × context growth**, not file reads or hook
instructions. The file-split and hook fixes shipped during round one were real
engineering improvements but addressed <1% of the cost. The honest fix is
workflow-level (compact/clear/start-fresh) and tooling-level (surface real
per-turn usage inside LoopDeck itself).

---

## Summary

Two investigations, one wrong conclusion, one correct one.

**Round one** (after the first "30M tokens in an hour" report) inspected the
Stop hook configuration and the memory-file sizes, concluded the cost was
caused by a hook instructing the model to re-read `.loopdeck/loops.md` and
`.loopdeck/decisions.md` every turn, and shipped fixes: lean hooks, archived
`loops.md` history, split a 120 KB `commands.rs` into a module tree. All real
improvements. None addressed the actual cost.

**Round two** (after the user shared a token-monitor screenshot and a second
session transcript) parsed the `usage` objects in the transcript JSONL and
discovered:

- The token counts in the transcript match the monitor exactly (14,144,231 for
  session 3) — so the transcript's `usage` data is the ground truth.
- **94% of tokens are `cache_read_input_tokens`** — the same growing context,
  replayed every turn at the cache discount (0.1×).
- The two round-one fixes together addressed <1% of the consumed tokens,
  because file reads happen *once per session*, while context replays happen
  *every turn*.

The single biggest lever is keeping conversations short: the model re-sends
its entire context on every turn, so a 123-turn session with a context that
grows from 29K to 143K tokens replays ~14M tokens of context. Multiply by
nine sessions in a day, and the count reaches 142M.

## Impact

| Dimension | Impact |
|-----------|--------|
| **Tokens consumed** | 142,208,631 in one day across 9 sessions. The two largest sessions alone account for 111M (78%). |
| **Dollar cost** | ~$75/day at Claude Sonnet rates. 94% of tokens are cache reads at 0.1×, so the bill is modest — but **if the cache ever invalidated** (model swap, long gap, system-prompt change), the same workload would cost ~$437. |
| **Quota / rate limits** | The token *count* is what hits provider quotas and rate limits, not the dollar cost. The cache discount does not help here. |
| **User-facing** | None at runtime — the app behaved correctly throughout. The cost was invisible until the user built a monitor. |
| **Engineering time** | Large. Two rounds of investigation; round one shipped two fixes that did not address the cost. |

## Timeline

All times approximate, 2026-07-19.

### Round one (misdiagnosis)

| Time | Event |
|------|-------|
| Session | User reports ~30M tokens in <1 hour, suspects the memory files are being sent multiple times. |
| ~T+0 | Inspect `.loopdeck/hooks/` and the Stop hook config. Find `loops.md` is 101 KB / 1653 lines and `decisions.md` is 50 KB — ~38K tokens of memory files, re-read every turn by hook instruction. |
| ~T+10m | **First diagnosis (wrong):** attribute the bleed to the Stop hook's "read loops.md/decisions.md" re-injection. Ship Fix #1: rewrite the hook to be append-only + dirty-flag-gated; archive `loops.md` history (−89%); de-dup heartbeats. |
| ~T+45m | Ship Fix #2: split `commands.rs` (2954 lines / ~30K tok) into a 7-file `commands/` module tree. |

### Round two (correction)

| Time | Event |
|------|-------|
| ~T+6h | User reports the problem persists and shares a screenshot from a token-monitor tool they built, plus a second session transcript (934 KB JSONL, 238 lines). |
| ~T+6h+5m | Parse the transcript's `usage` objects. Find 123 unique turns with real billing data: `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`. |
| ~T+6h+10m | **Cross-reference with the screenshot:** the transcript's totals (in 563K / cache 13.5M / out 118K = 14.14M) match the monitor exactly. The transcript is the ground truth. |
| ~T+6h+15m | **Revised diagnosis:** 94% of tokens are cache reads. The two round-one fixes addressed <1%. The dominant cost is context replay across long sessions. |
| ~T+6h+30m | Find an adjacent UX bug that masks the issue: `runStartLoop` in `AgentPanel.tsx` doesn't clear the visible conversation when starting a new loop while already viewing `"active"`, so users see the old conversation persist (and the LoopDeck workflow implicitly encourages long, growing conversations). Fix the bug. |

## Root Cause

### The real driver: context replay across long sessions

Every turn re-sends the **entire growing conversation** to the model. For
session 3 (the transcript in evidence):

| Turn | Context size (tokens) |
|------|-----------------------|
| 1    | 29,298 |
| 30   | 105,577 |
| 60   | 119,056 |
| 100  | 136,566 |
| 123  | 143,324 |

Summing the per-turn context across all 123 turns gives ~14M tokens — just for
re-sending history the model already has cached. Multiply across nine sessions
in a day:

| Session | Total tokens | Cache % | Est. cost |
|---|---|---|---|
| Add bounds for untrusted work | 63.2M | 96% | ~$30 |
| Resolve project-scoped IPC | 47.6M | 96% | ~$23 |
| Implement interruption recovery | 14.1M | 95% | ~$7.50 |
| …7 more sessions | ~17M | — | ~$15 |
| **Total** | **142M** | **94%** | **~$75** |

The cache absorbs most of the cost (cache reads are billed at 0.1×), which is
why the dollar total is "only" ~$75 rather than ~$437. But the **token count**
is what the user sees in their monitor, what hits provider quotas, and what
would balloon to hundreds of dollars if the cache ever invalidated.

### The adjacent UX bug: `runStartLoop` doesn't clear the view

`AgentPanel.tsx:runStartLoop` only reloaded the visible turns when switching
*from* an archive view to `"active"`. When the user was already on `"active"`
and clicked "Start next loop", the frontend skipped the reload — so the old
conversation stayed on screen while the backend correctly archived it and
spawned a fresh process. The model started with empty context (correct), but
the human saw the previous loop's turns mixed with the new prompt (confusing),
which made the workflow implicitly encourage working in one long, growing
conversation.

### Why round one was wrong

Round one measured `tool_result` **byte counts** in the transcript (how many
characters each file read returned) and concluded the memory files were the
cost. The actual billing data — the `usage` object on every assistant turn —
was in the same transcript the whole time, but round one never parsed it.
`tool_result` bytes tell you what the model *read*; `usage` tells you what
the model *paid for*. Those are very different numbers when cache replay
dominates.

Specifically, round one confused three things:

1. **"The hook instructs re-reads" ≠ "the model re-reads."** The Stop hook
   told the model to read the memory files every turn, but in the actual
   session the memory files were each read **once** (23% of cost), not
   repeatedly. The instruction's existence was treated as evidence of the
   cost.
2. **File reads ≠ context replays.** `commands.rs` cost ~30K tokens when
   read, but it was read once or twice per session — a rounding error against
   14M tokens of per-turn context replay.
3. **Char counts ≠ billed tokens.** Round one estimated token cost from file
   byte sizes (~4 chars/token), which has nothing to do with how the provider
   bills. The real unit is the `usage` object's `input` / `cache_read` /
   `output` breakdown, multiplied by the per-category price.

## The 5 Whys

**1. Why did the user consume 142M tokens in a day?**
Because each of ~9 sessions ran many turns (the largest had hundreds), and
every turn re-sent the entire growing conversation context — the model paid
for that replay every time, even though caching discounted it 10×.

**2. Why did the sessions run so many turns?**
Because nothing in LoopDeck's workflow told the user (or the model) to compact
or start fresh. LoopDeck's "Start next loop" appends to a growing transcript,
and a frontend bug (`runStartLoop` not clearing the view) made it look like
the old conversation was still active, reinforcing the "keep going in one
thread" behavior.

**3. Why didn't round one catch this?**
Because round one measured the wrong thing. It counted `tool_result` bytes
(file reads) instead of parsing the `usage` objects (billed tokens). The
ground truth was in the same transcript file the whole time.

**4. Why did the wrong measurement feel plausible?**
Because the user's initial report ("it sends the memory files multiple times")
pointed at the memory files, and the hook config did instruct re-reads — so
the hypothesis was self-confirming. The harder step of parsing actual billing
data was deferred until after fixes shipped.

**5. Why is context replay the dominant cost and not file reads?**
Because of a structural asymmetry: a file is read once into context, then
re-played on every subsequent turn until the conversation ends. A 30K-token
file read once costs 30K tokens; the same file in a 100-turn session costs
~3M tokens of replay. **The unit of cost is turn × context size, not file
size.** Round one optimized file size; the lever is turn count and context
size at the point of each turn.

## Action Items

| # | Action | Status | Type |
|---|--------|--------|------|
| 1 | Rewrite `loopdeck-stop-hook.py` to be append-only (no "read loops.md" instruction) | ✅ Done *(real improvement; <0.1% of cost)* | Fix |
| 2 | Add missing `loopdeck-dirty-flag.py` PreToolUse creator so Stop gating works | ✅ Done | Fix |
| 3 | Archive `loops.md` History (90 KB → `loops-archive.md`); de-dup `decisions.md` heartbeats | ✅ Done | Fix |
| 4 | Split `commands.rs` (2954 lines) into a `commands/` module tree | ✅ Done *(real improvement; ~0.02% of cost)* | Fix |
| 5 | Add "Context Discipline" convention to `CLAUDE.md`/`AGENTS.md` (don't re-read) | ✅ Done | Prevention |
| 6 | **Fix `runStartLoop` to always reload turns after the backend archives** — so starting a new loop clears the visible conversation | ✅ Done | Fix |
| 7 | **Add real per-turn usage telemetry to LoopDeck's agent runtime** (input / cache_read / output / running context size), surfaced in the UI — the user built an external monitor because LoopDeck didn't expose this | ⏳ Proposed | Tooling |
| 8 | **Auto-compact or prompt to start fresh when context crosses a threshold** (e.g. 80K tokens) — the single biggest token lever | ⏳ Proposed | Tooling |
| 9 | **Surface "session length × context size" economics in the LoopDeck UI** so users see when a session is getting expensive before the bill arrives | ⏳ Proposed | Tooling |

## What We Got Wrong (Diagnostically)

- **Measured the wrong thing.** Round one counted `tool_result` bytes — what
  the model *read*. The actual cost lives in the `usage` object — what the
  provider *billed*. Both were in the same transcript. Parsing `usage` from
  the start would have ended round one in minutes.
- **Confused cache-replay with file-reads.** The 30K-token `commands.rs` read
  was visible and large, so it felt like the lever. But it was read once;
  its contribution to a 14M-token session was rounding error. The real cost
  was the ~14M tokens of context replayed every turn, which doesn't show up
  in any `tool_result` — it's in the prompt itself.
- **Accepted the user's framing instead of measuring.** "It sends the memory
  files multiple times" was a reasonable hypothesis, and the hook config
  supported it, so fixes shipped before the billing data was checked. The
  memory files were each read once (23% of cost) — real overhead worth fixing,
  but not the 30M-token driver.
- **Estimated dollar cost from char counts.** "~4 chars per token" has nothing
  to do with provider billing, which is per-category (`input` / `cache_read` /
  `output`) with 10× discounts for cached reads. The real economics were
  invisible until the `usage` data was parsed.

## What We Got Right

- **Asked for the transcript.** Once the user shared the JSONL, the billing
  data was undeniable. The 94% cache-read share was visible immediately.
- **Asked for the monitor screenshot.** The user-built tool exposed per-session
  totals and made it obvious that the JSONL's `usage` was the ground truth
  (the numbers matched exactly). Cross-referencing the two confirmed the
  diagnosis.
- **Was honest about the misdiagnosis.** When the transcript contradicted the
  round-one framing, the correction was surfaced explicitly. The round-one
  fixes were kept (they were real improvements) but reframed as <1% of the
  cost, not the main fix.
- **Verified the refactor against a baseline.** Before splitting
  `commands.rs`, the test count (305 passing) and clippy warning count (5)
  were captured; after, both were re-checked via `git stash` and found
  identical.
- **Found the adjacent UX bug.** While investigating the cost, found that
  `runStartLoop` didn't clear the view — a real bug that reinforced the very
  "long growing conversation" behavior driving the cost. Fixed it.

## Lessons Learned

1. **For cost defects, parse the `usage` object, not the `tool_result` bytes.**
   A transcript carries the real billing breakdown on every assistant turn.
   Counting file-read bytes tells you what the model *looked at*; the `usage`
   object tells you what you *paid for*. They diverge dramatically when cache
   replay dominates — and cache replay almost always dominates in long
   sessions.
2. **The unit of token cost is `turn × context size`, not `file size`.** A
   large file read once is a one-time tax; the same file replayed across 100
   turns is a 100× tax. Optimizing file size (splitting modules) helps the
   one-time read; only reducing turn count or context size (compact / clear /
   fresh-session) moves the replay cost.
3. **Cache discounts hide the problem until they don't.** 94% of tokens being
   cache reads at 0.1× kept the dollar cost at ~$75 instead of ~$437 — but
   the token count still hits quotas and rate limits, and any cache
   invalidation would have quintupled the bill. Monitor the token count, not
   just the dollar total.
4. **UX bugs that hide "start fresh" encourage cost growth.** `runStartLoop`
   not clearing the view made each new loop feel like a continuation, so
   conversations grew unbounded. The visible behavior should match the
   backend's actual fresh-start semantics — if the backend archives, the UI
   should clear.
5. **Surface real usage in the product, not via an external tool.** The user
   built a monitor because LoopDeck didn't expose per-turn `usage`. That data
   is already in the transcript; surfacing it in-app (with a "context is at
   120K — consider starting fresh" nudge) is the highest-leverage tooling work
   here. Items 7–9 in Action Items are the real fix.

## References

- **Round-one fixes (real improvements, <1% of cost):**
  - `templates/hooks/loopdeck-stop-hook.py` (rewritten, append-only nudge)
  - `templates/hooks/loopdeck-dirty-flag.py` (new — PreToolUse dirty-flag creator)
  - `.loopdeck/loops.md` (History archived to `.loopdeck/loops-archive.md`, −89%)
  - `src-tauri/src/commands/{mod,state,composer,project,config_cmds,epics,agent}.rs` (split from `commands.rs`)
- **Round-two fix:**
  - `src/components/detail/AgentPanel.tsx:runStartLoop` — always reload turns
    from `active` after the backend archives; previously only reloaded when
    switching from an archive view
- **Evidence:**
  - Session 3 transcript (934 KB JSONL, 238 lines, 123 assistant turns) —
    `usage` totals: `input` 563,210 / `cache_read` 13,463,040 / `output`
    117,981 = 14,144,231 tokens; matches the user's monitor screenshot exactly
  - User-built token monitor: 142,208,631 tokens today across 9 sessions, 94%
    cache reads
- **Related:**
  - `docs/postmortem-duplicate-user-message.md` (2026-07-06) — same template;
    same theme of "measure the right layer before theorizing"
