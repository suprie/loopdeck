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
2. **Persistent conversation history + resume.** Every turn is appended to
   `.loopdeck/sessions/active.jsonl` and surfaced in a new **Agent** tab in
   `ProjectDetail`. On restart, the agent is re-spawned with `--resume <id>` so
   the model's own context is restored — "continue where we left off" works
   across app restarts, not just within one live process.

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
| P0 | **Persisted conversation** — every turn written to `.loopdeck/sessions/active.jsonl` and viewable in the UI |
| P0 | **Resume on restart** — re-spawn with `--resume <session_id>` so model context survives app restarts |
| P1 | An **Agent** tab in `ProjectDetail` showing transcript + free-form follow-up input + "Start next loop" |
| P1 | `agent_send_message` (free-form follow-up), `agent_reset_session` (fresh conversation) |

## Non-Goals

- **Streaming / token-by-token UI** — batch first; `send_message` returns one
  `AgentResponse` per turn. A spinner covers long turns. (Future PRD.)
- **Per-project agent config overrides** — global config only, as today.
- **Editing/replaying transcript** — history is append-only display; reset
  archives it.
- **Multi-agent orchestration within a project** — one session per project.

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

### Resume across restarts

- `ClaudeSession::spawn(project_path, agent_config, resume_session_id: Option<&str>)`
  — when `Some(id)`, pushes `--resume <id>` into the claude args.
- On Start, read the last `session_id` from the transcript; if present, pass it
  to `spawn`. The live process retains context within a run; `--resume` restores
  it after a restart.

### Conversation persistence (new file: `src-tauri/src/conversation.rs`)

```
.loopdeck/
└── sessions/
    ├── active.jsonl          # current conversation, one JSON object per line
    └── archive-<ts>.jsonl    # rotated when user resets
```

`ConversationTurn` (Serialize/Deserialize):

```jsonc
{
  "ts": "2026-07-03T12:00:00Z",
  "role": "user",                       // "user" | "assistant"
  "text": "...",
  "session_id": "abc123",               // present when known (assistant turns)
  "is_error": false,                    // assistant turns only
  "usage": { "input_tokens": 100, "output_tokens": 20, "total_cost_usd": 0.005 },
  "duration_ms": 1500
}
```

API: `load_conversation(path) -> Vec<ConversationTurn>`,
`last_session_id(path) -> Option<String>`, `append_turn(path, turn)`,
`archive_conversation(path)`.

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
| `agent_start_loop` | `path: String` | `AgentResponse` | Build next-loop prompt, send via shared pipeline |
| `agent_send_message` | `path, prompt: String` | `AgentResponse` | Free-form follow-up; same pipeline |
| `agent_get_conversation` | `path: String` | `Vec<ConversationTurn>` | Load transcript for Agent tab |
| `agent_reset_session` | `path: String` | `()` | Drop live process, archive transcript (next Start is fresh, no resume) |

**Shared send pipeline** (used by both `agent_start_loop` and
`agent_send_message`): `with_session` → `lock().await` → `append_turn(user)` →
`send_message` → `append_turn(assistant)` → return. The user turn is appended
*before* sending so a crash mid-turn still records intent.

---

## Implementation Plan

### Phase 1 — Backend: concurrency + spawn-with-resume (`claude_session.rs`)
- Extend `spawn` signature with `resume_session_id: Option<&str>`; push
  `--resume <id>` when set.
- `send_message`, `Drop`, stderr drain, timeout: unchanged.

### Phase 2 — Backend: session store + get-or-spawn helper
- New `src-tauri/src/conversation.rs` (`ConversationTurn`, load/append/archive).
- `AppState.claude_sessions` → `Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<ClaudeSession>>>>`.
- `with_session(path, closure)` helper: get-or-spawn `Arc`, spawn reads agent
  config (clear error if unset) + `last_session_id`, returns locked guard.

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
  matches, auto-call `agent_start_loop`. Render transcript bubbles + Start +
  free-form Send + New conversation. Spinner while busy; disable Send/Start
  while in flight.
- `ProjectDetail.tsx`: add `agent` tab (Bot icon); local `useState<DetailTab>`
  → store `detailTab`; render `<AgentPanel projectPath={project.path} />`.

### Phase 6 — Docs
- `.loopdeck/loops.md`: check off #4, #5, #6; add History entry covering Start
  button + conversation persistence + resume.

---

## Risks & Mitigations

| Risk | Mitigation |
|------|------------|
| **`--resume` + `--input-format stream-json` untested together** (research only validated stream-json *without* resume) | Add ignored integration test `test_session_resume_after_restart` (spawn → send → drop → re-spawn `--resume` → assert context retained). The `resume_session_id` param makes dropping resume a one-line change. |
| Holding `std::Mutex` across `.await` deadlocks | `with_session` returns the `Arc` from the `std::Mutex` guard scope *before* any `.await`; the guard is dropped first. Add a clippy-friendly comment marking the invariant. |
| Transcript grows unbounded across a long session | JSONL append is O(1); `archive_conversation` rotates on reset. Display paginates/truncates in the UI (future). |
| Process leak if app is force-quit mid-turn | Existing `Drop` already force-kills + reaps within a bounded window; map entry owns the `Arc` → last reference drops → child reaped. |
| Agent config unset → confusing spawn failure | `with_session` returns a clear `AppError::Agent("no agent config set; configure it in Settings")`. |

---

## Verification

- `cargo check` + `cargo clippy` clean on new code.
- `cargo test --lib claude_session -- --ignored --nocapture` — existing 3 plus
  the new resume test, against the live provider.
- `npm run tauri dev`:
  - Press Start on **two** projects → both turns run concurrently (distinct
    processes, neither blocks the other).
  - Two turns on the **same** project → second queues behind the first.
  - Agent tab shows transcript; closing + reopening the app resumes the
    conversation via `session_id`.

---

## Success Criteria

- [ ] Start button on `ProjectCard` spawns the agent and prompts for the next loop
- [ ] Multiple projects run concurrently; same-project turns serialize
- [ ] Every turn persists to `.loopdeck/sessions/active.jsonl`
- [ ] Restarting the app resumes the conversation via `--resume <session_id>`
- [ ] Agent tab shows transcript + allows free-form follow-up + reset
- [ ] `cargo check` / `cargo clippy` clean; ignored integration tests pass
- [ ] `loops.md` items #4, #5, #6 checked off

---

## Files Touched

- `src-tauri/src/claude_session.rs` (spawn signature)
- `src-tauri/src/conversation.rs` (new)
- `src-tauri/src/commands.rs` (state + 4 commands + `with_session` helper)
- `src-tauri/src/lib.rs` (register commands)
- `src/lib/tauri.ts`, `src/types/index.ts`, `src/store/appStore.ts`
- `src/components/dashboard/ProjectCard.tsx`, `src/components/dashboard/Dashboard.tsx`
- `src/components/detail/ProjectDetail.tsx`, `src/components/detail/AgentPanel.tsx` (new)
- `.loopdeck/loops.md`
