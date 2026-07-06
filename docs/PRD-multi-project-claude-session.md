# PRD — Multi-Project Claude Sessions

## Status: 📋 Proposed (2026-07-03)

---

## Overview

Wire the existing `ClaudeSession` (persistent `claude --input-format stream-json`
process) into the LoopDeck UI so that pressing **Start** on a project card spawns
the agent, prompts it for the **next loop** derived from `.loopdeck/loops.md`,
and drives the work via the `loopdeck-orchestrator` skill conventions.

Two capabilities that do not exist today are added on top:

1. **True parallelism across projects.** Pressing Start on several projects runs
   them concurrently — each project gets its own claude process and its own
   turn lock. Turns within a single project still serialize (one process, one
   stdin).
2. **Persistent conversation history (fresh start each time).** Every turn is
   appended to `.loopdeck/sessions/active.jsonl` and surfaced in a new **Agent**
   tab in `ProjectDetail`. Pressing **Start** always begins a **new**
   conversation — the previous transcript is archived and a fresh claude process
   is spawned. Within a run, follow-up turns reuse the live process's in-memory
   context; across restarts, Start begins fresh (context is re-established from
   `.loopdeck/loops.md` via the orchestrator prompt, not resumed into the model).

---

## Problem Statement

`ClaudeSession` is built and validated (cross-turn context retention proven
against a live provider), but it is **unreachable from the UI**:

- No Tauri command calls `ClaudeSession::send_message`. The wiring planned in
  `.loopdeck/loops.md` items #4 (per-project turn lock), #5 (`with_session`
  helper), and #6 (`agent_send_message` command) was never built.
- `AppState.claude_sessions` is a `Mutex<HashMap<PathBuf, ClaudeSession>>`
  using a single `std::sync::Mutex`. Because `send_message` is `async` and takes
  seconds-to-minutes, this would either (a) serialize every project behind one
  global lock, or (b) deadlock/corrupt by holding a `std::Mutex` across `.await`.
- There is no Start button on `ProjectCard`, no Agent view, and no persisted
  transcript — closing the app loses the conversation entirely.

---

## Goals

| Priority | Goal |
|----------|------|
| P0 | A **Start** CTA on `ProjectCard` that spawns the agent for that project and prompts for the next loop |
| P0 | **Parallel across projects** — distinct claude processes run concurrently; same-project turns queue behind a per-project lock |
| P0 | **Start = fresh conversation** — each Start archives the prior transcript and spawns a new claude process (never resumes) |
| P0 | **Persisted conversation** — every turn written to `.loopdeck/sessions/active.jsonl` and viewable in the UI |
| P1 | An **Agent** tab in `ProjectDetail` showing transcript + free-form follow-up input + "Start next loop" |
| P1 | `agent_send_message` (free-form follow-up on the live session), `agent_reset_session` (archive + drop, no prompt) |

## Non-Goals

- **Streaming / token-by-token UI** — batch first; `send_message` returns one
  `AgentResponse` per turn. A spinner covers long turns. (Future PRD.)
- **Auto-resume into model context** — Start always begins a fresh conversation;
  the model never receives `--resume`. Continuity within a run comes from the
  live process; across runs, the orchestrator prompt re-seeds context from
  `.loopdeck/` memory files.
- **Per-project agent config overrides** — global config only, as today.
- **Editing/replaying transcript** — history is append-only display; Start/reset
  archives it.
- **Multi-agent orchestration within a project** — one live session per project.

---

## Proposed Design

### Concurrency model — the core fix

```
AppState {
    config: Mutex<GlobalConfig>,                                           // unchanged
    claude_sessions: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<ClaudeSession>>>>,
}
```

Two lock layers with two different scopes:

| Lock | Type | Held for | Purpose |
|------|------|----------|---------|
| Outer (map) | `std::sync::Mutex` | microseconds | Insert/lookup of an `Arc` in the map. **Never** held across `.await`. |
| Inner (per-project) | `tokio::sync::Mutex` | one full turn (seconds–minutes) | Serializes turns **within** a project (one stdin, one process). Different projects take different inner locks → true parallelism. |

This realizes `loops.md` #4 (turn lock) and #5 (`with_session`). A
`with_session(path, |session| async { ... })` helper does get-or-spawn and hands
the caller a locked guard; the `Arc<ClaudeSession>` never escapes the helper.

### Concurrent command handling — what happens when a turn is in flight

A live session has three states: **absent** (no process), **idle** (alive,
between turns, lock free), and **busy** (a `send_message` `.await` is reading
stdout, lock held). Pressing Start or Send in each state:

| State | Start | Send |
|---|---|---|
| Absent | Spawn fresh + send prompt ✅ | Error: "no active session; press Start" |
| Idle | `try_lock` Ok → drop old, archive, spawn fresh, send ✅ | `lock().await` → send ✅ |
| **Busy** | **`try_lock` Err → reject "agent is busy, wait for the current turn"** | **`lock().await` → queue behind the running turn** |

The two commands use **different acquire strategies on the same lock**:
`agent_start_loop` uses `try_lock()` (reject if held), `agent_send_message` uses
`lock().await` (wait). Consequence: **no turn is ever force-interrupted.** Since
neither command can kill a running `send_message`, the dangling-transcript
problem (user turn appended via append-before-send, then its matching assistant
turn killed) cannot occur.

**Start sequence (busy-safe):** `try_lock` the per-project arc → if `Err`,
reject immediately → if `Ok`, the old session is provably idle (lock free ⇒ no
in-flight turn), so drop it (map remove → `Drop` closes stdin → EOF → claude
exits → child reaped) → `archive_conversation` → spawn fresh → insert new arc →
send next-loop prompt. The successful `try_lock` is the proof that replacing the
session is safe.

**Known, bounded race (documented, not fixed):** if a Send is *queued* (sitting
on `lock().await` against the old arc) at the exact instant Start swaps in a
fresh session, the queued Send runs against the orphaned old session and errors
when that session is dropped. This requires firing a Send and a Start within
milliseconds of a turn completing; the frontend prevents it by disabling buttons
while busy. The worst case is one clean, surfaced error — no corruption, no UB.
Out of scope to harden further for a single-user desktop app; the per-project
lock exists primarily to prevent two concurrent writes to one stdin (the
genuinely dangerous case).

### Start always begins a fresh conversation

- `ClaudeSession::spawn(project_path, agent_config)` — unchanged signature; no
  `--resume` flag. Every Start spawns a brand-new claude process.
- On Start: if a live session exists for the project, drop it (map remove →
  `Drop` reaps the child); archive `active.jsonl` → `archive-<ts>.jsonl`; spawn
  fresh; begin a new `active.jsonl`. Then send the next-loop prompt.
- Continuity model (Start vs Send differ by design):
  - **Start** always begins fresh: drops any live session, archives the
    transcript, spawns a new process **without** `--resume`. Context is re-seeded
    from `.loopdeck/loops.md` + `decisions.md` via the orchestrator prompt.
  - **Send** (`agent_send_message`) continues the existing conversation: reuses
    the live process within a run, and after an app restart (no live process)
    re-spawns claude **with** `--resume <last_session_id>` so a follow-up keeps
    the model's context. So `--resume` stays in the codebase, used only by Send.

### Conversation persistence (new file: `src-tauri/src/conversation.rs`)

```
.loopdeck/
└── sessions/
    ├── active.jsonl          # current conversation, one JSON object per line
    └── archive-<ts>.jsonl    # rotated when user Start/resets
```

`ConversationTurn` (Serialize/Deserialize):

```jsonc
{
  "ts": "2026-07-03T12:00:00Z",
  "role": "user",                       // "user" | "assistant"
  "text": "...",
  "session_id": "abc123",               // present when known (assistant turns); display only
  "is_error": false,                    // assistant turns only
  "usage": { "input_tokens": 100, "output_tokens": 20, "total_cost_usd": 0.005 },
  "duration_ms": 1500
}
```

API: `load_conversation(path) -> Vec<ConversationTurn>`,
`append_turn(path, turn)`, `archive_conversation(path)`,
`last_session_id(path) -> Option<String>`. (`last_session_id` is used by Send's
`with_session` to `--resume` after a restart; Start never resumes, so the
session id is otherwise kept on turns for display/debugging only.)

### The "next loop" prompt (built in Rust)

Scan `.loopdeck/loops.md` raw text for the first unchecked `- [ ]` under
`## Next Steps` (the existing `memory::parse_loops` drops the checked/unchecked
distinction, so read the raw file here). Prompt body:

> You are working on this LoopDeck project. Use the `loopdeck-orchestrator`
> skill conventions. Read `.loopdeck/loops.md` for full context. The next
> unchecked step is: "<step>". Implement it. When done, update
> `.loopdeck/loops.md` (mark the step `[x]`, refresh `## Current`) and append
> any architectural decisions to `.loopdeck/decisions.md` per the memory
> convention.

Fallback when no unchecked step exists: "review `.loopdeck/loops.md`, propose
and start the next loop."

### IPC commands (net-new)

| Command | Args | Returns | Description |
|---------|------|---------|-------------|
| `agent_start_loop` | `path: String` | `AgentResponse` | **`try_lock`** the per-project arc; if busy, reject. Else drop old idle session, archive transcript, spawn fresh, send next-loop prompt |
| `agent_send_message` | `path, prompt: String` | `AgentResponse` | **`lock().await`** (queues behind a running turn); errors if no live session (tells user to press Start) |
| `agent_get_conversation` | `path: String` | `Vec<ConversationTurn>` | Load transcript for Agent tab |
| `agent_reset_session` | `path: String` | `()` | **`try_lock`** (reject if busy); drop live process + archive transcript without sending a prompt |

**Send pipeline** (`agent_send_message`, uses `lock().await`): requires a live
session — if none, returns a clear "no active session; press Start" error.
Otherwise: get the `Arc<tokio::sync::Mutex<ClaudeSession>>` → `lock().await`
(queues behind any running turn) → `append_turn(user)` → `send_message` →
`append_turn(assistant)` → return. The user turn is appended *before* sending so
a crash mid-turn still records intent.

**Start pipeline** (`agent_start_loop`, uses `try_lock`): acquire the per-project
arc's lock non-blockingly — if `Err`, reject "agent is busy, wait for the current
turn". If `Ok`, the old session is provably idle, so: drop it (map remove →
`Drop` reaps the child), `archive_conversation` (rotate `active.jsonl`), spawn a
new `ClaudeSession`, insert the new arc into the map, then run the same
append/send/append sequence with the next-loop prompt. `try_lock` succeeding is
the precondition that makes the swap safe.

---

## Implementation Plan

### Phase 1 — Backend: concurrency (`claude_session.rs`)
- `spawn` signature unchanged — no `--resume`. Every Start spawns fresh.
- `send_message`, `Drop`, stderr drain, timeout: unchanged.

### Phase 2 — Backend: session store + access helpers
- New `src-tauri/src/conversation.rs` (`ConversationTurn`, load/append/archive).
- `AppState.claude_sessions` → `Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<ClaudeSession>>>>`.
- Session access helpers, split by lock strategy (the invariant from the
  concurrency section above):
  - `get_session(path)` — lookup existing `Arc`, or `None`. Used by Send.
  - `spawn_fresh(path)` — `try_lock`-guarded (Start/Reset call this only after a
    successful `try_lock` proves the old session is idle): read agent config
    (clear error if unset), spawn new `ClaudeSession`, evict+drop any prior arc
    from the map (`Drop` reaps the child), insert the new arc.
- Send: `get_session` → `None` ⇒ "press Start" error; `Some` ⇒ `lock().await` →
  send pipeline.
- Start/Reset: `try_lock` on the existing arc (if any) ⇒ `Err` ⇒ "busy" error;
  `Ok` or no arc ⇒ `spawn_fresh` (Start then runs the send pipeline with the
  next-loop prompt; Reset just archives).

### Phase 3 — Backend: four Tauri commands (`commands.rs`, `lib.rs`)
- `agent_start_loop`, `agent_send_message`, `agent_get_conversation`,
  `agent_reset_session`. Register all in `lib.rs` `invoke_handler`.

### Phase 4 — Frontend wiring (`lib/tauri.ts`, `types/index.ts`, store)
- `types/index.ts`: `"agent"` on `DetailTab`; `ConversationTurn`, `AgentResponse`,
  `UsageInfo`.
- `lib/tauri.ts`: four typed wrappers.
- `store/appStore.ts`: lift `ProjectDetail`'s local `activeTab` into `detailTab`
  + `setDetailTab`; add `pendingAgentStart` + setter (dashboard Start lands on
  the Agent tab and auto-fires).

### Phase 5 — Frontend UI
- `ProjectCard.tsx`: prominent **Start** CTA (Play icon, primary) above the
  icon-tile row. `onStart` → `setSelectedProject` + `setDetailTab("agent")` +
  `setPendingAgentStart(path)`. Wire `onStart` from `Dashboard.tsx`.
- New `src/components/detail/AgentPanel.tsx` (mirrors `DecisionsPanel`/`LoopsPanel`
  loading/empty/error states): load transcript on mount; if `pendingAgentStart`
  matches, auto-call `agent_start_loop`. Render transcript bubbles, a "Start
  next loop" button (always begins a fresh conversation, archiving the prior),
  and a free-form input + Send (continues the live session; disabled with a hint
  when no session is live). Spinner while busy; disable actions while in flight.
- `ProjectDetail.tsx`: add `agent` tab (Bot icon); local `useState<DetailTab>`
  → store `detailTab`; render `<AgentPanel projectPath={project.path} />`.

### Phase 6 — Docs
- `.loopdeck/loops.md`: check off #4, #5, #6; add History entry covering Start
  button + conversation persistence (fresh-start semantics).

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| Holding `std::Mutex` across `.await` deadlocks | `get_session`/`spawn_fresh` return the `Arc` (and for Start, drop the old `Arc`) from the `std::Mutex` guard scope *before* any `.await`; the guard is dropped first. Add a clippy-friendly comment marking the invariant. |
| Transcript grows unbounded across a long session | JSONL append is O(1); `archive_conversation` rotates on every Start/reset. Display paginates/truncates in the UI (future). |
| Process leak if app is force-quit mid-turn | Existing `Drop` already force-kills + reaps within a bounded window; map entry owns the `Arc` → last reference drops → child reaped. |
| Agent config unset → confusing spawn failure | `spawn_fresh` returns a clear `AppError::Agent("no agent config set; configure it in Settings")`. |
| Losing model context across restart feels abrupt | By design — Start re-seeds from `.loopdeck/loops.md` + `decisions.md` via the orchestrator prompt, so a fresh conversation is not "from scratch"; it carries the project's accumulated memory. Documented in the Agent tab empty state. |

> Note: dropping `--resume` removes the previously-flagged risk that
> `--resume` + `--input-format stream-json` were untested together. The research
> docs only validated stream-json *without* resume; this design stays entirely
> within that validated path.

---

## Verification

- `cargo check` + `cargo clippy` clean on new code.
- `cargo test --lib claude_session -- --ignored --nocapture` — existing 3 tests
  against the live provider.
- `npm run tauri dev`:
  - Press Start on **two** projects → both turns run concurrently (distinct
    processes, neither blocks the other).
  - Two turns on the **same** project → second queues behind the first.
  - Agent tab shows transcript; pressing Start again archives the prior and
    begins a fresh conversation.

---

## Success Criteria

- [ ] Start button on `ProjectCard` spawns the agent and prompts for the next loop
- [ ] Multiple projects run concurrently; same-project turns serialize
- [ ] Start while a turn is in flight is rejected ("agent is busy"); Send queues behind a running turn
- [ ] No turn is ever force-interrupted (no dangling transcript entries)
- [ ] Every turn persists to `.loopdeck/sessions/active.jsonl`
- [ ] Start always begins a fresh conversation (archives prior transcript, no `--resume`)
- [ ] Agent tab shows transcript + allows free-form follow-up
- [ ] `cargo check` / `cargo clippy` clean; ignored integration tests pass
- [ ] `loops.md` items #4, #5, #6 checked off

---

## Files Touched

- `src-tauri/src/claude_session.rs` (no signature change — confirmed fresh spawn)
- `src-tauri/src/conversation.rs` (new)
- `src-tauri/src/commands.rs` (state + 4 commands + `get_session`/`spawn_fresh` helpers)
- `src-tauri/src/lib.rs` (register commands)
- `src/lib/tauri.ts`, `src/types/index.ts`, `src/store/appStore.ts`
- `src/components/dashboard/ProjectCard.tsx`, `src/components/dashboard/Dashboard.tsx`
- `src/components/detail/ProjectDetail.tsx`, `src/components/detail/AgentPanel.tsx` (new)
- `.loopdeck/loops.md`
