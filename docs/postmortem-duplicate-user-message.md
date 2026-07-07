# Postmortem — Duplicate User Message in Chat Transcript

## Status: ✅ Resolved (2026-07-06)

A user's sent message rendered **twice** in the chat transcript — instantly on
send, persisting forever. The fix was a one-line markup bug. Finding it took
five wrong hypotheses across a multi-hour debugging session. This document is
the first postmortem in the project and exists primarily to capture *why* a
trivial bug survived five rounds of misdiagnosis, so the same diagnostic
discipline gap doesn't recur.

---

## Summary

Every user message appeared twice in the transcript. The doubling was
**instant** (present on the first render after send) and **permanent** (never
collapsed, never went away). The data layer was correct throughout: the
on-disk transcript held exactly one user turn and one assistant turn per
exchange, and the in-memory `turns` array matched it. The bug was a pure
render-layer defect in `TurnBubble` (`src/components/detail/Chat.tsx`): user
turns rendered their text through two code paths that both executed, so the
message appeared twice inside a single bubble.

## Impact

| Dimension | Impact |
|-----------|--------|
| **User-facing** | Every sent message visually duplicated, making the transcript read as if the user had sent everything twice. Confusing and broken-looking. |
| **Data integrity** | None — the persisted transcript and the agent's context were always correct. Pure display bug. |
| **Agent behavior** | None — the agent received one copy of each message. No double-execution, no doubled tool calls. |
| **Engineering time** | Large. Four failed fix attempts shipped before the root cause was found (see *Timeline* and *Lessons Learned*). |

## Timeline

All times approximate, 2026-07-06.

| Time | Event |
|------|-------|
| ~15:40 | `@`-mention autocomplete feature shipped. User reports "the sender is doubled" with a screenshot showing two identical user bubbles. |
| ~15:50 | **Hypothesis 1 (failed):** Assumed a React-state race between the optimistic `setTurns` insert and the canonical transcript reload. Added a render-time `dedupConsecutiveUserTurns` that collapsed adjacent identical user turns. *Did not fix it.* |
| ~16:10 | **Hypothesis 2 (failed):** Broadened the dedup to trim any trailing user turn with no assistant turn after it (mirroring the backend's orphan filter). *Did not fix it.* |
| ~16:25 | **Hypothesis 3 (failed):** Broadened again to drop any user turn whose text duplicated an earlier user turn's text, regardless of position. Added a diagnostic `console.log` of the full `turns` array. User reported it still doubled. |
| ~16:40 | **Hypothesis 4 (failed):** Replaced the optimistic insert with a new `pendingUserText` field on the streaming store, rendered as an ephemeral bubble outside `turns`. Reasoning: this would make a duplicate "structurally impossible." *Still doubled.* |
| ~17:00 | User asked to debug it themselves. Equipped them with `console.log` instrumentation at three sites: `[dbg]` (full turn list), `[bubbles]` (groupLoopRuns output), `[TurnBubble]` (per-bubble). |
| ~17:20 | User returned with the decisive evidence: `[dbg]` showed `turnsLen: 4` (correct), `[bubbles]` showed `count: 4` (correct), but `[TurnBubble]` logged the doubled message. **This proved the data and the list were correct — the doubling was inside one bubble.** |
| ~17:25 | **Root cause found.** `TurnBubble` rendered user-turn text twice: once via the fallback ternary's `else` arm, once via a trailing `{isUser && (...)}` block. Removed the duplicate path. Fixed. |

## Root Cause

`TurnBubble` (`src/components/detail/Chat.tsx`) had two independent render
paths for user-turn body text, and for a user turn **both executed**:

```tsx
// Path A — the body's fallback ternary. For a user turn, !isUser is false,
// so this hits the else arm and renders the text:
{!isUser && turn.blocks?.length ? (
  <BlockList blocks={turn.blocks} />
) : (
  <>
    {!isUser && <ThinkingBlock .../>}
    {!isUser && <ToolList .../>}
    {!isUser ? (
      <div><Markdown>{turn.text}</Markdown></div>   // assistant
    ) : (
      <p>{turn.text}</p>                            // ← user: renders ONCE
    )}
  </>
)}

// Path B — a trailing block immediately below, unconditionally for user turns:
{isUser && (
  <p>{turn.text}</p>                                // ← user: renders AGAIN
)}
```

For a user turn, `isUser` is `true`, so Path A's ternary falls to its `else`
arm (rendering the text), **and** Path B's `isUser &&` guard is satisfied
(rendering the text a second time). Net effect: every user bubble contained
two `<p>` elements with the same message.

The bug was introduced when the assistant body grew a blocks-vs-legacy
fallback structure. The fallback ternary was written to handle *both* roles
(as evidenced by the `!isUser ? assistant : user` arm), but the original
user-specific block (Path B) was never removed — so user text had two homes.

### Why the data looked fine

The doubling was entirely within one `TurnBubble` instance. The `turns` array
held one user turn per message, `groupLoopRuns(turns)` produced one item per
turn, and `TurnBubble` was invoked once per item. Every layer above the
component was correct. Only *inside* the component did the text render twice.
This is why every data-layer hypothesis failed: there was no data-layer bug.

## The 5 Whys

**1. Why did the user see their message twice?**
Because `TurnBubble` rendered user-turn text through two code paths (a fallback
ternary's `else` arm and a trailing `{isUser && ...}` block), both of which
executed for user turns.

**2. Why did both paths execute?**
Because the fallback ternary was written to be role-aware (`!isUser ?
assistant : user`) but the separate, older user-only block (Path B) was never
removed when the ternary took over user rendering. The two paths coexisted
with overlapping responsibility and no single owner.

**3. Why wasn't the overlap caught when it was introduced?**
There was no test asserting "a user turn renders its text exactly once," and
no visual review caught it because the two `<p>`s stacked identically and read
as one duplicated paragraph rather than as an obvious layout break. The
defect was silent under casual inspection.

**4. Why did the fix take five attempts instead of one?**
Because the diagnostic process fixated on the *data and state layers*
(optimistic inserts, reload races, dedup logic) and never inspected the
*render layer* until the user instrumented `TurnBubble` directly. Each
hypothesis was plausible, shipped on theory alone, and was confirmed "fixed"
only by `tsc --noEmit` passing — not by observing the actual rendered output.
The repeated pattern was: theorize → edit → typecheck → declare victory →
user reports it still broken.

**5. Why did theory override observation for four full rounds?**
Because the early screenshot analysis (correctly) reported "two user bubbles,"
which anchored the investigation on "there must be two user turns somewhere."
Once `console.log` evidence contradicted that (turnsLen correct, bubbles count
correct), the real cause was found in minutes. **The lesson: instrument the
system and read what it says before theorizing about why.** A single
`console.log` inside the suspect component would have ended this in round one.

## Action Items

| # | Action | Status | Type |
|---|--------|--------|------|
| 1 | Fix the duplicate render path in `TurnBubble` (collapse to one `isUser ?` branch) | ✅ Done — `962a069` | Fix |
| 2 | Remove diagnostic `console.log`s added during debugging (`[dbg]`, `[bubbles]`, `[TurnBubble]`) | ✅ Done — folded into `962a069` | Cleanup |
| 3 | Add a render test asserting a user turn renders its text exactly once (guards against Path A/B regression) | ⏳ Pending | Prevention |
| 4 | Adopt a debugging protocol: before shipping a fix for a visible defect, add a log/inspection at the *rendered output* layer and confirm the defect is gone — not just that types check | ⏳ Proposed | Process |

## What We Got Wrong (Diagnostically)

- **Confused "plausible" with "confirmed."** Each of the four failed fixes
  addressed a real, plausible race — but none was *verified* as the cause
  before the fix shipped. `tsc --noEmit` passing was treated as evidence of a
  fix; it only proves types compile.
- **Anchored on the wrong layer.** The screenshot said "two bubbles," so the
  search stayed in the state/data layer for four rounds. The defect was in the
  component markup one level down.
- **Shipped on theory, not observation.** No fix attempt included a log that
  would have *distinguished* the hypothesis from alternatives. The diagnostic
  that finally worked (`[TurnBubble]` per-bubble log) was the first one placed
  at the actual render site.

## What We Got Right

- **The user took over debugging.** When the maintainer asked to instrument it
  themselves and came back with `[dbg]` + `[bubbles]` + `[TurnBubble]` output,
  the root cause was identified within minutes. Delegating the observation
  broke the theory-loop.
- **The on-disk transcript was checked early.** `grep -c` on `active.jsonl`
  proved the backend wrote one user turn — which correctly ruled out a whole
  class of backend double-write hypotheses, even if it didn't pinpoint the
  render bug.
- **The eventual fix was minimal and correct.** One branch restructure, no
  speculative defense-in-depth, no leftover dedup machinery. The failed
  attempts' churn (dedup helpers, `pendingUserText` store field) was cleaned
  up rather than left to rot.

## Lessons Learned

1. **Observe before you theorize.** When a visible defect is reported, the
   first debugging step should be a log/inspection at the layer that produces
   the visible output — not a hypothesis about the layer two steps up.
2. **"Types check" ≠ "the bug is fixed."** Compilation and type-safety are
   necessary but tell you nothing about runtime render behavior. Confirm a UI
   fix by observing the UI (or a render of it).
3. **A contradiction in the evidence is the most valuable signal.** When
   `turnsLen: 4` and `bubbles count: 4` but the screen shows a duplicate, the
   defect is *narrower* than the data — pursue that contradiction rather than
   re-litigating the data.
4. **Co-located render paths with overlapping responsibility are a smell.**
   The moment a ternary grows an `else` arm that duplicates a sibling block's
   job, one of them is wrong. Refactor to a single owner per concern.

## References

- Fix commit: `962a069` — *feat(chat): add @-mention file/folder autocomplete
  to composer* (the TurnBubble fix was folded into the feature commit)
- Suspect component: `src/components/detail/Chat.tsx`, `TurnBubble`
- Related (now-removed) speculative machinery: `dedupConsecutiveUserTurns`,
  `pendingUserText` field on `src/store/streamingState.ts`
