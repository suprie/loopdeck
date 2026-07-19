---
prd: prd-token-economics
status: proposed
priority: P1
target: 0.2.0
description: >
  Make LoopDeck's real token cost visible and controllable. Surface per-turn
  usage (including cache reads — the 94% of tokens currently invisible in the
  UI), warn before sessions get expensive, and offer one-click compaction /
  fresh-start so the workflow that caused a 142M-token day can't recur
  silently.
---

# PRD — Token Economics

## Overview

LoopDeck already records per-turn usage data — `input_tokens`,
`cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`,
and `total_cost_usd` arrive on every assistant turn from the `claude` CLI.
The backend persists most of it to the transcript; the frontend renders a
slice of it (non-cached input + output + dollar total) under each assistant
bubble.

That slice hides the dominant cost. A real LoopDeck development session
consumed **14.1M tokens in 123 turns**, of which **95% were cache reads** —
the same growing conversation context replayed on every turn. Across nine
sessions in one day the total reached **142,208,631 tokens**. The user
discovered this only by building an external monitor; LoopDeck's own UI
showed `$0.0008 total` per turn while the actual billed-equivalent context
replay was orders of magnitude larger. (Full forensic in
`docs/postmortem-runaway-token-usage.md`.)

This PRD makes the cost visible where it happens and gives the user a cheap
escape valve. It does **not** introduce billing, quotas, metering, or any
cloud-side accounting — everything is computed locally from the `usage`
data the CLI already returns.

## Problem Statement

### The dominant cost is invisible in the UI

`UsageInfo` (frontend type) carries only `input_tokens`, `output_tokens`, and
`total_cost_usd`. The largest token category by far — `cache_read_input_tokens`
— is dropped somewhere between the CLI response and the rendered bubble. A
turn that logs `input: 579 / cache_read: 122,112 / output: 1,152` in the
transcript renders in the UI as `579 → 1,152 tok · $0.0008`. The 122K of
replayed context is the actual cost driver and is nowhere on screen.

### `total_cost_usd` understates the bill on cache-heavy providers

The dollar figure shown today is computed by the `claude` CLI using its own
pricing assumptions. On providers where cache reads are billed at a discount
(e.g. 0.1×) the per-turn figure looks negligible — but the **token count**
still counts against quotas, rate limits, and (on some providers) a separate
per-token budget. Users optimizing against the displayed dollar total will
miss the lever that actually matters.

### Nothing tells the user a session is getting expensive

Context grows monotonically within a session: turn 1 = 29K, turn 60 = 119K,
turn 123 = 143K (real numbers from the postmortem transcript). The cost of
each turn is roughly `current_context_size × turn_count_remaining`, but
nothing in the UI surfaces the growth or suggests compacting / starting
fresh. The user finds out only when an external monitor trips an alarm.

### The workflow quietly encourages unbounded growth

Until the `runStartLoop` fix (decision 2026-07-19), starting a new loop did
not clear the visible conversation — so the natural workflow was to keep
working in one growing thread. Even with that bug fixed, there is no
in-product affordance for "this session is getting heavy, start fresh" or
"compact the running context." The user has to remember to do it themselves,
and nothing reminds them.

## Goals

| Priority | Goal |
|---|---|
| P0 | Surface the **real** per-turn token cost in the UI, including cache reads and cache writes, so what the user sees matches what the provider bills |
| P0 | Surface the **running session total** (tokens + cost) so cumulative spend is visible without an external monitor |
| P0 | Surface **current context size** and its growth trend so the user can see a session getting heavy before it gets expensive |
| P1 | Offer a one-click **compact** action (forward the `/compact` control to the running claude process) when context crosses a soft threshold |
| P1 | Offer a one-click **start fresh** action that archives the current conversation and spawns a new session without `--resume` |
| P1 | Persist per-turn usage to the transcript so historical sessions are analyzable in-product (today only `input`/`output`/`cost` are persisted; cache fields are dropped) |
| P2 | Add a per-project and per-day rollup view so users can see which projects / sessions dominate cost |
| P2 | Surface a "this session has crossed *N* M tokens" banner to make expensive sessions obvious |

## Non-Goals

- **No billing, invoicing, or payment integration.** This is local-only cost
  awareness computed from `usage` data the CLI already returns.
- **No quotas or hard limits enforced by LoopDeck.** The user is free to
  ignore every nudge. This PRD adds visibility and affordances, not caps.
- **No cloud-side accounting or sync.** Everything is in-process and
  on-disk.
- **No multi-provider pricing engine.** Use the `total_cost_usd` the CLI
  returns; display the raw token categories alongside it without trying to
  recompute dollars ourselves.
- **No automatic context compaction without user action.** Auto-compact is a
  possible future direction but is explicitly out of scope for 0.2.0; the
  user must trigger it. (See Risks.)
- **No change to the model, the agent runtime, or the permission system.**
  This PRD only adds observability and workflow affordances around the
  existing turn pipeline.
- **No replacement for the external token monitor.** If a user wants
  cross-session/cross-day aggregation today they can still build one; this
  PRD just makes it less necessary.

## Product Contract

### What "cost" means in LoopDeck

For each assistant turn, LoopDeck records (or will record) five numbers from
the CLI's `usage` object:

| Field | Meaning | Billed at |
|---|---|---|
| `input_tokens` | Non-cached prompt tokens (the new content this turn) | 1× |
| `cache_creation_input_tokens` | Tokens written to the prompt cache this turn | ~1.25× (provider-specific) |
| `cache_read_input_tokens` | Tokens served from the prompt cache this turn | ~0.1× (provider-specific) |
| `output_tokens` | Tokens the model produced | ~5× (provider-specific) |
| `total_cost_usd` | Dollar total as computed by the CLI | n/a |

The single best proxy for "this session is getting expensive" is
**`sum(current_context_size)` across turns**, where
`current_context_size = input_tokens + cache_creation_input_tokens +
cache_read_input_tokens`. This is the metric the UI must surface and trend.

### What the user sees

Three layers of visibility, from local to global:

1. **Per-turn** (under each assistant bubble) — the existing
   `input → output tok · $cost` row, **augmented** with the cache breakdown
   so a turn that reads 122K of cached context shows it.
2. **Per-session** (Agent panel header) — running totals for the current
   conversation: total tokens, total cost, current context size, and a
   sparkline / trend indicator showing context growth across the last *N*
   turns.
3. **Per-project / per-day** (Dashboard or a new "Cost" tab) — rollups across
   sessions for a project, and across projects for the current day. Replaces
   the need for the external monitor.

### What the user can do

Two escape valves, both already supported by the existing runtime:

1. **Compact** — sends the `compact` control request to the running claude
   process. The model summarizes the conversation so far and the context
   resets to the summary. The live process and session id survive. This is
   the lower-friction option and is appropriate when the current thread is
   still relevant.
2. **Start fresh** — archives the current `active.jsonl` and spawns a new
   claude process without `--resume` (exactly what the existing
   `agent_start_loop` / `agent_reset_session` commands do). Higher friction
   (loses in-process context) but a hard reset on context size. Appropriate
   when moving to an unrelated task.

LoopDeck **suggests** one or the other when context crosses a threshold but
**does not perform either automatically** in 0.2.0.

## Functional Requirements

### FR1 — Capture full usage data end-to-end

**Today:** the CLI returns `input_tokens`, `cache_creation_input_tokens`,
`cache_read_input_tokens`, `output_tokens`, `total_cost_usd` (and more) on
every assistant turn. The Rust `Usage` struct persists only a subset; the
frontend `UsageInfo` type carries only `input_tokens`, `output_tokens`,
`total_cost_usd`. The cache fields — the dominant cost — are dropped.

**Required:** carry the full `usage` object from the CLI response through
`AgentResponse` → transcript persistence → `ConversationTurn` → Tauri IPC →
frontend `UsageInfo`, without loss. Specifically, `UsageInfo` gains
`cache_read_input_tokens: number` and `cache_creation_input_tokens: number`
(both defaulting to 0 for older transcripts).

**Acceptance:** a turn whose transcript logged `cache_read: 122,112` renders
in the UI with that number visible; loading a pre-0.2.0 transcript does not
crash and renders the new fields as 0.

### FR2 — Render real per-turn cost in the chat UI

**Today:** `Chat.tsx` renders `{input} → {output} tok · ${cost}` under each
assistant bubble.

**Required:** the meta row shows all four token categories in a compact,
scannable form. Design goal: a user glancing at the row should immediately
see "this turn replayed 122K of cached context." Suggested layout (final
copy in implementation):

```
↑122,112 cache · 579 in → 1,152 out · $0.0008 · 2.1s
```

The cache-read count is the headline number when present; non-cached input
is secondary. Color or weight the cache number when it crosses a per-turn
threshold (e.g. > 50K) to draw the eye.

**Acceptance:** on the postmortem transcript, the dominant-cost turns (the
ones that read 100K+ of cached context) are visually obvious in the chat
view without hovering or expanding.

### FR3 — Surface running session totals and context growth

**Today:** the Agent panel shows the conversation but no aggregate cost.

**Required:** a session-cost header above the transcript shows:

- **Total tokens this session** (sum of all categories across turns)
- **Total cost this session** (sum of `total_cost_usd`)
- **Current context size** (`input + cache_creation + cache_read` of the most
  recent turn) — the number that predicts the cost of the *next* turn
- A small trend indicator (sparkline or `↗ growing`/`→ steady`/`↘ compacted`)
  over the last ~20 turns of context size

**Acceptance:** at any point during a session, the user can see at a glance
how big the context has grown and roughly how many tokens the session has
consumed — without an external tool.

### FR4 — Suggest compaction at a soft threshold

**Today:** nothing prompts the user to compact.

**Required:** when the current context size crosses a configurable soft
threshold (default **80,000 tokens**), the session header shows a
non-blocking banner: "Context is at *N* tokens — consider compacting or
starting fresh." The banner offers two buttons:

- **Compact** → sends `compact` as a control request to the running claude
  process (the same mechanism LoopDeck already uses for permission and
  AskUserQuestion parking). On success the next turn's context drops to the
  summary size; the trend indicator reflects it.
- **Start fresh** → invokes the existing `agent_reset_session` flow
  (archives `active.jsonl`, drops the live process, next Start spawns
  without `--resume`).

The banner is **dismissable** and **does not auto-reopen** for the rest of
the session once dismissed. The threshold is configurable in agent settings;
`0` disables the banner entirely.

**Acceptance:** a session that grows past the threshold shows the banner
within one turn of crossing it; clicking Compact visibly reduces the next
turn's context size; dismissing the banner keeps it dismissed.

### FR5 — Persist the full usage to the transcript

**Today:** `ConversationTurn::assistant` records a `Usage`-shaped value, but
the cache fields are dropped on the Rust side before persistence.

**Required:** the persisted turn in `active.jsonl` (and archives) carries
the full `usage` object, including `cache_read_input_tokens` and
`cache_creation_input_tokens`. Old transcripts (pre-0.2.0) load with those
fields defaulting to 0.

**Acceptance:** after a 0.2.0 session, `grep cache_read active.jsonl`
returns the field on every assistant turn; loading a 0.1.0-era archive
renders without errors and shows the new fields as 0.

### FR6 — Per-project and per-day rollup view

**Today:** no aggregate cost view. The Activity Feed shows events across
projects but not token totals.

**Required:** a new view (suggested: `/cost` route, or a "Cost" tab on the
Dashboard) showing:

- **Per-project, last 7 days:** total tokens, total cost, session count,
  largest session. Sorted by total tokens descending.
- **Per-day, last 14 days:** total tokens, total cost. Simple bar chart.
- **Per-session, current project:** table of sessions with turn count,
  total tokens, total cost, peak context size.

All aggregations are computed client-side from the persisted `usage` data
(FR5); no new backend state.

**Acceptance:** the view reproduces the headline numbers from
`docs/postmortem-runaway-token-usage.md` for any session recorded after
0.2.0 — a user can answer "which session consumed the most tokens today?"
without leaving the app.

## Phases

### Phase 1 — Usage data plumbing (P0, FR1 + FR5)

Carry the full `usage` object end-to-end and persist it. No UI change beyond
not crashing on the new fields. This is the foundation: every later phase
depends on the cache fields being available in the frontend.

### Phase 2 — Per-turn UI (P0, FR2)

Update `Chat.tsx`'s meta row to show all four token categories. Iterate on
layout/copy until the dominant cost is visually obvious at a glance.

### Phase 3 — Session header (P0, FR3)

Add the session-cost header with totals, current context size, and growth
trend. This is the "no external monitor needed" milestone.

### Phase 4 — Compaction nudge (P1, FR4)

Wire the `compact` control request through the existing control-protocol
plumbing and add the threshold banner. Start conservative (default 80K;
dismissable; no auto-action) to validate that compaction works as expected
before considering any automation in a later release.

### Phase 5 — Rollup view (P2, FR6)

Add the `/cost` view. Pure client-side aggregation over persisted `usage`;
no new IPC commands strictly required, though a `get_usage_rollup` command
may be added for performance if the client-side computation gets expensive.

## Acceptance Criteria

- On a fresh 0.2.0 session, every assistant turn's cache-read token count is
  visible in the chat UI.
- The session header shows running totals that match what an external
  monitor would compute from the same transcript.
- A session that crosses the 80K threshold shows the compaction banner
  within one turn; clicking Compact reduces the next turn's context size.
- The persisted transcript (post-0.2.0) contains `cache_read_input_tokens`
  and `cache_creation_input_tokens` on every assistant turn.
- The `/cost` view reproduces the postmortem's per-session token totals for
  any session recorded in 0.2.0.
- Pre-0.2.0 transcripts load without errors; the new fields render as 0.

## Risks and Mitigations

| Risk | Mitigation |
|---|---|
| **Auto-compact (out of scope here) could lose context mid-task.** A future PRD might propose compacting automatically at a hard threshold; that risks dropping detail the model needed. | 0.2.0 only suggests compaction and requires a click. Automation is explicitly deferred. |
| **The CLI's `usage` shape is not stable across versions.** Fields may be added, renamed, or restructured. | Treat unknown fields as opaque passthrough; only the five named fields are semantically depended on. Add a serde fallback that zeroes missing fields rather than failing the turn. |
| **`compact` as a control request may not be supported by every provider / CLI build.** | Feature-detect at first use; if the control is rejected, fall back to "Start fresh" and disable the Compact button with an explanatory tooltip. |
| **Per-turn UI clutter.** Four numbers + cost + duration under every bubble is a lot. | Use weight/color to make the dominant number the headline; collapse the secondary numbers into a smaller, dimmer run. Keep the row one physical line. |
| **Cache-read counts may alarm users even when dollar cost is low (0.1×).** | Always pair token counts with the dollar total. The intent is *awareness*, not panic; the trend indicator ("context is growing") is more actionable than the raw count. |
| **Persisting new fields bloats the transcript.** | Two additional integer fields per assistant turn is negligible (tens of bytes). No mitigation needed. |

## Verification Strategy

- **Unit / integration (Rust):** extend the existing `ConversationTurn`
  serialization tests to cover the new fields; add a deserialization test
  using a pre-0.2.0 fixture to confirm missing fields default to 0.
- **Type safety (TS):** `npx tsc --noEmit` clean with the widened
  `UsageInfo`.
- **Manual smoke:** run the postmortem scenario — drive a session to 100K+
  context, confirm the chat UI shows the cache-read count, confirm the
  session header matches an external monitor's total, cross the 80K
  threshold, click Compact, confirm the next turn's context drops.
- **Regression:** load a pre-0.2.0 `active.jsonl` (e.g. one of the existing
  `.loopdeck/sessions/archive-*.jsonl` files), confirm it renders without
  errors and shows the new fields as 0.
- **No new lint debt:** clippy + `tsc` warning count unchanged or improved.

## Implementation Order

1. **FR1 + FR5 (Phase 1)** — widen `Usage` / `UsageInfo`; plumb through
   `AgentResponse` → `ConversationTurn` → IPC → TS. Land this even before
   any UI change so transcripts start capturing the data.
2. **FR2 (Phase 2)** — per-turn UI. Small, visible, validates the data path.
3. **FR3 (Phase 3)** — session header. The "no external monitor needed"
   milestone; promote in release notes.
4. **FR4 (Phase 4)** — compaction nudge. Higher-risk (new control flow);
   ship after the observability phases have proven the data.
5. **FR6 (Phase 5)** — rollup view. Nice-to-have; can slip past 0.2.0 if
   needed without weakening the release.

## References

- `docs/postmortem-runaway-token-usage.md` — the forensic that motivated
  this PRD (142M tokens / day, 94% cache reads)
- Existing per-turn UI: `src/components/detail/Chat.tsx` meta row (lines
  ~357–362, ~598–603)
- Existing `UsageInfo` type: `src/types/index.ts:223`
- Existing `compact`-class control plumbing: `claude_session.rs` control
  request/response (same path used by `AskUserQuestion` and manual
  approval)
- Existing archive/reset flow: `commands/agent.rs::spawn_fresh` and
  `agent_reset_session`
