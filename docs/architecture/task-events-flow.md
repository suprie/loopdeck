# Task Events (TaskUpdate / TaskCreate) — Annotated Flow Diagram

Trace of how task lifecycle events (create / update / complete / delete) flow
from the agent's Task/TodoWrite tool through the backend, over the streaming
channel, into frontend state, and onto the TaskPanel.

## TL;DR

**`TaskCreate` does not exist as an event type.** The entire task lifecycle
flows through one `TaskUpdate` channel event. "Created" is just one possible
value of `TaskRecord.status`, classified by substring-matching the tool-result
text — not a distinct event.

## Legend

- 👤 **USER-VISIBLE** — you see or interact with this
- ⚙️ **INTERNAL** — backend plumbing, invisible to you
- ◆ **FORK** — a branch point
- ❌ **ERROR/REJECT** — a failure path (none in this flow)

## Flow

```
👤  During a streaming turn, the agent runs its Task/TodoWrite tool
    │
    ▼   ─────────────────────── RUST  (claude CLI stream) ───────────────────────
⚙️  Claude emits NDJSON:  { "type": "user", "tool_use_result": { "task": {...} } }
    │   modeled as StreamEvent::ToolResult                 [agents.rs:262-284]
    │     └─ tool_use_result.task → TaskWire { id, subject }   (verb NOT in payload)
    │                                                            [agents.rs:437-454]
    │
    ▼   ◆ the read loop sees TWO consumers on the same event:
    │
    │   ┌─────────────────────────────┐    ┌──────────────────────────────────┐
    │   │  CONSUMER 1 — live channel  │    │  CONSUMER 2 — persisted acc.     │
    │   │  claude_session.rs:1119-1127│    │  agents.rs:716-721               │
    │   └──────────────┬──────────────┘    └──────────────┬───────────────────┘
    │                  │                                  │
    │                  ▼                                  ▼
    │   ⚙️ extract_task_from_tool_result     ⚙️ extract_task_from_tool_result
    │       (shared — same fn, can't drift)      (SAME shared fn)
    │                                         [agents.rs:537-566]
    │                  │                                  │
    │                  │   ◆ branching inside extractor:  │
    │                  │   ┌──────────────────────────────┤
    │                  │   │ not ToolResult?   → None     │
    │                  │   │ no task payload?  → None     │
    │                  │   │ id+subject empty? → None     │
    │                  │   │ else → mine verb (↓) ────────┤
    │                  │   └──────────────────────────────┘
    │                  │                                  │
    │                  ▼                                  ▼
    │   ⚙️ task_status_from(text)            ⚙️ push into acc.tasks: Vec
    │       ◆ substring match on lowercased     (arrived-order log)
    │           tool-result text:                              [agents.rs:716-721]
    │       ┌─────────────────────────────────────┐
    │       │ "created"            → "created"     │
    │       │ "updated"/"modified" → "updated"     │
    │       │ "completed"/"done"   → "completed"   │
    │       │ "deleted"/"removed"  → "deleted"     │
    │       │ (else)               → "updated"  ◄──┼─ default fallback
    │       └─────────────────────────────────────┘
    │                     [agents.rs:515-528]
    │                  │                                  │
    │                  ▼                                  ▼
    │   ⚙️ channel.send(ClaudeEvent::TaskUpdate{task})   ⚙️ acc.finish() folds into
    │       → serializes as                                 AgentResponse.tasks
    │         { "type":"task_update", "task":{...} }       [agents.rs:728-746]
    │       [agents.rs:123-127;  claude_session.rs:1121]            │
    │                  │                                            ▼
    │                  │                                  ⚙️ ConversationTurn::assistant(
    │                  │                                        …, response.tasks)
    │                  │                                     → appended to active.jsonl
    │                  │                                     [commands.rs:2204, 2264,
    │                  │                                              2407, 2463]
    │                  │                                            │
    │                  │                                  ⚙️ tasks field is
    │                  │                                     #[serde(skip_serializing_if
    │                  │                                       = empty)]  → old transcripts
    │                  │                                     still load
    │                  │                                     [conversation.rs:156-161]
    │                  │
    ▼   ───────────────────────────── FRONTEND ─────────────────────────────────
👤  channel.onmessage: case "task_update"                [AgentPanel.tsx:508-517]
    │   ◆ NOTE: does NOT push to streamingBlocks (no transcript row)
    │           → only mutates the tasks map (TaskPanel is the sole surface)
    │
    ▼
⚙️  useStreamingState.getState().applyTask(path, task)   [streamingState.ts:145-159]
    │   ◆ branching on task.status:
    │
    │      ┌──────────────────────────────────────────────┐
    │      │ "deleted"  → delete next[task.id]            │  ← drops from map
    │      │ anything   → next[task.id] = task            │  ← last-write-wins by id
    │      │   else        (created/updated/completed)    │
    │      └──────────────────────────────────────────────┘
    │
    │   net effect: a created→updated→completed sequence
    │   COLLAPSES to one row with status "completed"
    │
    ▼
👤  TaskPanel re-renders                                 [TaskPanel.tsx]
    │   • subscribes to byPath[path].tasks                [:31]
    │   • sorts Object.values() by numeric id             [:36-44]
    │   • returns null when empty (hidden)                [:46]
    │   ◆ statusVisual / statusBadgeCls switch:           [:117-128]
    │       "completed" → emerald ✓
    │       "created"   → primary +
    │       "deleted"   → destructive ✕
    │       "updated"/default → amber ✎

    ── tasks map lifecycle (when it gets reset) ──────────────────────────────
    ⚙️  cleared at 3 points in AgentPanel.tsx:
        • :441  turn begin        → patch({ tasks: {} })  (fresh todo set each turn)
        • :603  3.5s after result  → linger so you see "5/5 done", then clear
        • :624/:639  fallback/catch → immediate clear
```

## The single-consumer insight

The same `StreamEvent::ToolResult` event is consumed **twice**, in parallel, by
the same shared extractor (`extract_task_from_tool_result`):

1. **Live path** (`claude_session.rs:1119`) → emits `ClaudeEvent::TaskUpdate`
   over the channel → drives the TaskPanel in real time.
2. **Persisted path** (`agents.rs:716`, inside `ResponseAccumulator::ingest_event`)
   → pushes into `acc.tasks` → folded into `AgentResponse.tasks` at `finish()`
   → written to `active.jsonl` as `ConversationTurn.tasks`.

Because both call the *same* extractor, the live view and the persisted
transcript can never disagree about what tasks existed in a turn.

## Differences between "TaskUpdate" and "TaskCreate"

| Aspect | TaskUpdate | TaskCreate |
|---|---|---|
| Exists as event type? | Yes — `ClaudeEvent::TaskUpdate` / `{ type: "task_update" }` | **No** — does not exist |
| Origin | Shared extractor on every `StreamEvent::ToolResult` | N/A |
| How "create" is represented | As `status === "created"` (a data value), from `task_status_from` | N/A |
| Frontend handling | `AgentPanel.tsx:508` → `applyTask` → `tasks[id]` | Only a dead `break` guard at `:500` |
| Persistence | `ConversationTurn.tasks` in `active.jsonl` | N/A |

## Two dead-code oddities worth flagging

1. **`AgentPanel.tsx:500`** — `if (event.name === "TaskCreate") break;` inside
   the `tool_use` case. No backend path emits a `tool_use` named `"TaskCreate"`,
   so this never fires. (Stray `console.log` debug lines at 501–502 are
   leftovers too.)

2. **`claude_session.rs:1106`** — `if name == "ToolUpdate" { continue; }` looks
   like it was meant to suppress the Task/TodoWrite tool_use block (mirroring
   the frontend guard), but the string is `"ToolUpdate"`, not `"TaskCreate"`.
   So today the Task tool_use block actually leaks through as a `ToolUse`
   event — harmless because the real signal is the `task_update` from the tool
   *result*, which arrives separately.

## One-line mental model

> One event (`task_update`), four statuses (`created|updated|completed|deleted`)
> mined from tool-result text, upserted by id into a map that the TaskPanel
> renders — with a parallel persisted copy folded into the transcript turn.

## Files referenced

- `src/types/index.ts` — `ClaudeEvent` union, `TaskRecord`
- `src/store/streamingState.ts` — `applyTask`, `beginTurn`, `tasks` state
- `src/components/detail/AgentPanel.tsx` — `task_update` handler, dead `TaskCreate` guard, reset points
- `src/components/detail/TaskPanel.tsx` — rendering + status visual switch
- `src-tauri/src/agents.rs` — `ClaudeEvent` enum, `StreamEvent::ToolResult`, `TaskWire`, `task_status_from`, `extract_task_from_tool_result`, `ResponseAccumulator`
- `src-tauri/src/claude_session.rs` — live `TaskUpdate` emission, tool_use suppression
- `src-tauri/src/conversation.rs` — `TaskRecord` struct, `ConversationTurn.tasks`
- `src-tauri/src/commands.rs` — persistence call sites into `ConversationTurn::assistant`
