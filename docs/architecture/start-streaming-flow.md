# Start Streaming — Annotated Flow Diagram

Trace of what happens when you tap **"Start next loop"** in the agent panel,
from the tap through every backend fork, back to the frontend transcript reload.

## Legend

- 👤 **USER-VISIBLE** — you see or interact with this
- ⚙️ **INTERNAL** — backend plumbing, invisible to you
- ◆ **FORK** — a branch point
- ❌ **ERROR/REJECT** — a failure path
- 🎯 **TERMINAL** — the event that ends the turn

## Flow

```
👤  TAP "Start next loop"                                  [src/components/detail/AgentPanel.tsx:888]
    │
    ▼
👤  runStartLoop                                          [AgentPanel.tsx:648]
    │  ◆ viewing archived conversation?
    │      ├─ YES → switch to active + reload turns       [agentGetConversationById]
    │      └─ NO  ─────────────────────────┐
    └──────────────────────────────────────┘
                                            │
                                            ▼
⚙️  runStreamingTurn(undefined)                            [AgentPanel.tsx:429]
    │   • busyRef.current = true  (sync race-guard)        [:431]
    │   • Channel<ClaudeEvent> created + onmessage wired  [:460–606]
    │   ◆ prompt === undefined?
    │      ├─ yes → agentStartLoopStreaming  ◄── START     [tauri.ts:266]
    │      └─ str → agentSendMessageStreaming ◄── SEND     [tauri.ts:285]
    └──────────────────────┬───────────────────────────────┘
                           │
                           ▼   ─────────────────────────── RUST ───────────────────────────
⚙️  invoke("agent_start_loop_streaming")                   [registered: lib.rs:72–75]
    │
    ▼
⚙️  agent_start_loop_streaming                             [commands.rs:1177]
    │   • validate path exists                             [:1180]
    │   • build_next_loop_prompt(repo_path) ─────┐
    │   • start_fresh_and_record_streaming       │
    └────────────────────────────────────────────┼──────────┘
                                                 │
                                                 ▼   ◆ PROMPT BRANCH
⚙️  build_next_loop_prompt                                 [commands.rs:1749]
    │   match next_unchecked_loop_step(path)               [commands.rs:1775]
    │      ├─ Some(step) → "implement step: {step}"        [:1758]
    │      └─ None       → "propose & start next loop"     [:1765]
    │   (scans .loopdeck/loops.md for first `- [ ]`
    │    under ## Next Steps)                              [commands.rs:1775–1794]
    │
    ▼
⚙️  start_fresh_and_record_streaming                       [commands.rs:2277]
    │
    ▼
⚙️  spawn_fresh                                            [commands.rs:2147]
    │   ◆ 1. try_lock live session                         [:2162]
    │         ├─ HELD ─┐
    │         └─ free ─┤
    │   ◆ 2. drop old session arc (child reaped via Drop)  [:2170]
    │   ◆ 3. archive_conversation(path)  → rotate          [:2180]  (conversation.rs)
    │           active.jsonl aside
    │   ◆ 4. no agent config?
    │         ├─ YES → ❌ Err: no agent config             [:2196–2200]
    │         └─ NO  ─┐
    │   ◆ 5. ClaudeSession::spawn(resume=None, fresh)      [:2210]
    │                 │
    │                 ▼
    │   ┌─────────────┴─────────────┐
    │   │  record user turn         │  conversation::append_turn   [conversation.rs]
    │   │  send_message_streaming   │  (crash-safe write first)
    │   └─────────────┬─────────────┘
    └─────────────────┘
                      │
    ┌─────────────────┘  (the held branch)
    ▼
❌  Err "agent is busy"                                    [commands.rs:2162]
    → done, reject (does NOT queue; Send path would queue via lock().await)
```

```
   ─────────────────────── RUST  claude_session.rs:927 ───────────────────────
⚙️  send_message_streaming
    │
    ▼
⚙️  loop { tokio::select! (biased) }                       [claude_session.rs:988–1008]
    │   ◆ three racers:
    │
    │   ┌─────────────────────────┬──────────────────────────────────────┐
    │   │ 👤 interrupt_rx (Stop)   │ Result(partial) + Err               │  [:1012–1032]
    │   │                         │ "turn interrupted by user"           │
    │   │                         │ (session SURVIVES — can resume)      │
    │   ├─────────────────────────┼──────────────────────────────────────┤
    │   │ ⚙️ sleep(READ_TIMEOUT)   │ Err "claude produced no stdout       │  [:1002]
    │   │                         │  for Ns — assuming stuck"            │
    │   ├─────────────────────────┼──────────────────────────────────────┤
    │   │ ⚙️ stdout.read_line ────┼─▶ parse line                        │
    │   └─────────────────────────┴──────────────────────────────────────┘
    │                                  │
    │                       ◆ branch on parsed line:
    │                                  │
    │       ┌────────────┬─────────────┴────────────┬──────────────────┐
    │       ▼            ▼                          ▼                  ▼
    │   ❌ n == 0    ⚙️ ControlRequest           ⚙️ Assistant        ⚙️ result
    │   Err "closed  → answer_control_request     match ContentBlock:   event?
    │   before       continue                     • Text → TextDelta     │
    │   result"                                   • Think→ ThinkingDelta  │ yes → break
    │   [:1009]    [:1057–1071]                   • ToolUse → emit       │ [:1120]
    │                                              (skip AskUserQuestion) │
    │                                              [:1076–1112]           │
    │                                                                      ▼
    │                                                                  🎯 break
    │
    ▼  (after loop)
⚙️  emit terminal ClaudeEvent::Result                      [claude_session.rs:1137–1145]
⚙️  clear interrupt / question / permission slots         [:1161–1163]  (every exit path)
    │
    │   ┌──────────────────────────────────────────────────────────────────────┐
    │   │  ⚙️ answer_control_request — 4 arms, in order   [claude_session.rs:343]│
    │   │                                                                        │
    │   │   ① tool == AskUserQuestion?                          [:352]           │
    │   │         │ yes → park on question_slot                                  │
    │   │         │       👤 emit AskUserQuestion ─► UI card   [:442–559]         │
    │   │         │       await agent_answer_question                           │
    │   │         │       write back allow + updatedInput.answers               │
    │   │         ▼ no                                                          │
    │   │   ② policy.decide → destructive? (rm -rf, sudo…)     [:364]            │
    │   │         │ yes → ❌ hard-Deny immediately                               │
    │   │         ▼ no                                                          │
    │   │   ③ mutating tool? (Bash/Edit/Write/WebFetch/        [:371–377]        │
    │   │           NotebookEdit)                                                │
    │   │         │ yes → 👤 emit PermissionRequest{pending} ─► approval card   │
    │   │         │       park on permission_slot               [:587+]           │
    │   │         │       👤 await agent_answer_permission (Allow/Deny)          │
    │   │         ▼ no                                                          │
    │   │   ④ read-only → auto-allow synchronously                              │
    │   └──────────────────────────────────────────────────────────────────────┘
    │
    ▼   ───────────────────────────── FRONTEND ─────────────────────────────
👤  channel.onmessage  switch(event.type)                  [AgentPanel.tsx:460–606]
    │
    ├─ text_delta / thinking_delta  → coalesce into streaming bubble         [:465–485]
    ├─ tool_use                     → push activity block                     [:490–520]
    ├─ task_update                  → applyTask                               [:525–535]
    ├─ permission_request           → ◆ decision === "pending"?               [:540–560]
    │                                   ├─ yes → 👤 show approval card
    │                                   └─ resolved → clear card
    ├─ ask_user_question            → 👤 show question card                   [:565–575]
    │
    └─ 🎯 result  ◄──────────────────── TERMINAL                              [AgentPanel.tsx:587–604]
            • busy = false
            • clear pending cards
            • reload transcript from disk
            • .finally → clear streaming bubble + busy flag
```

## How to read the color-coding

- 👤 marks every place **you** are involved — the tap, the Stop button, approval cards, question cards, and the final transcript you see reload. There are only **four** user-visible touchpoints in the whole flow: the initial tap, Stop (interrupt), the permission/question cards that park the loop, and the result that reloads your view.
- ⚙️ is everything in between — the orchestration, the spawn, the read loop, the event parsing. This is the bulk of the flow and it's all invisible to you.
- ◆ marks the eight fork points. The three that actually change *your* experience:
  1. **"agent busy"** (`spawn_fresh:2162`) — a second Start tap is rejected, not queued. You'd see nothing happen except maybe a flash.
  2. **Permission parking** (`answer_control_request:343`) — this is why output appears to "freeze" when claude wants to run a mutating tool. The read loop is literally suspended on a oneshot channel waiting on you.
  3. **The terminal `Result`** (`claude_session.rs:1137`) — the single event that unfreezes everything. If it never fires (claude crash), you stay stuck busy until the stuck-timeout racer wins.
- ❌ are the three rejection paths: busy, no config, and destructive-tool hard-deny. All three are silent on the streaming side — they just end the turn.

## Key asymmetry

**Start rejects-if-busy (❌), but Send queues (⚙️ `lock().await`).** So if you tap Start while a turn is live you get rejected; if you type into the chat while a turn is live, your message waits its turn. Same runner, opposite policy on contention.

## One-line mental model

> **Start** = archive old transcript → spawn a fresh claude process (reject if busy) → feed it the next unchecked step from `loops.md` → stream events over a channel, parking for your approval whenever the agent reaches for a mutating tool, until a terminal `Result` event fires and the transcript reloads.

## Files referenced

- `src/components/detail/AgentPanel.tsx`
- `src/components/detail/Chat.tsx`
- `src/lib/tauri.ts`
- `src/types/index.ts`
- `src-tauri/src/lib.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/claude_session.rs`
- `src-tauri/src/permission.rs`
- `src-tauri/src/conversation.rs`
