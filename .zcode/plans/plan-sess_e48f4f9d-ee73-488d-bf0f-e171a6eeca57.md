## Root cause

Claude exposes **four separate tools** — `TaskCreate`, `TaskUpdate`, `TaskList`, `TaskGet` (confirmed in `docs/researchs/claude-code-request-with-skills.json`). The backend's task extractor (`extract_task_from_tool_result`, agents.rs:537) only fires on `tool_use_result.task.{id,subject}` — the shape a **`TaskCreate` result** returns. 

A **`TaskUpdate`** call carries its state change in the **tool_use `input`** (`{taskId, status}`), *not* in the result payload. So for every update the extractor returns `None`, no `task_update` event fires, and the panel freezes at the last "created" status.

**Empirical proof** (`.loopdeck/sessions/active.jsonl`): one turn ran 16 `TaskCreate` + 32 `TaskUpdate` tool_uses, but the persisted `tasks[]` contains only 8 `status:"created"` entries and **zero** updates.

Two secondary bugs in the same path (flagged in `docs/architecture/task-events-flow.md`):
- `claude_session.rs:1106` filters `name == "ToolUpdate"` — a typo; should be `"TaskUpdate"` (so today the `TaskUpdate` tool_use row leaks through as a noisy generic activity row).
- `AgentPanel.tsx:500-502` has a dead `TaskCreate` guard + leftover `console.log` debug lines.

## Fix

Two signals, two sources — mirroring Claude's two-tool design:

| Tool | Reliable signal | Extracted from |
|---|---|---|
| `TaskCreate` | id + subject | the **result** (`tool_use_result.task`) — *already works* |
| `TaskUpdate` | id + status (+ optional subject) | the **tool_use `input`** (`{taskId, status}`) — *new* |

### Backend (`src-tauri/src/agents.rs`)
1. Add `extract_task_from_tool_use(name: &str, input: &serde_json::Value) -> Option<TaskRecord>`:
   - For `name == "TaskUpdate"`: parse `taskId`, `status` (`pending`/`in_progress`/`completed`/`deleted`), and optional `subject` (usually absent → empty string). Return the record.
   - Anything else (including `TaskCreate`, whose input has no id): `None`.
2. In `ResponseAccumulator::ingest_event`'s `ContentBlock::ToolUse` arm (agents.rs:667): call the new helper and push to `self.tasks` — covers both the streaming and `parse_response` persistence paths.
3. Tests: add `test_extract_task_from_tool_use` (TaskUpdate input → `in_progress`) and extend the accumulator test so a create (result) + update (tool_use) both land.

### Backend (`src-tauri/src/claude_session.rs`, streaming loop ~1092-1114)
4. Inside the `ContentBlock::ToolUse { name, input }` arm: when `name == "TaskUpdate"`, parse via the new helper and emit `ClaudeEvent::TaskUpdate { task }` (mirrors how `extract_task_from_tool_result` is emitted today).
5. Suppress the generic `TaskUse` activity row for `TaskUpdate` (correct the dead `name == "ToolUpdate"` → `name == "TaskUpdate"`, and `continue`). The TaskPanel is the dedicated surface; a `› TaskUpdate · {json}` row would be noise — same rationale as the existing `AskUserQuestion` suppression.

### Frontend (`src/store/streamingState.ts`, `applyTask` ~145-159)
6. **Merge instead of replace**, so a status-only `TaskUpdate` (empty subject) keeps the subject from the prior create:
   ```ts
   next[task.id] = { id: task.id,
                     subject: task.subject || prev?.subject || "",
                     status: task.status };
   ```
   (`deleted` still removes from the map — unchanged.)

### Frontend (`src/components/detail/TaskPanel.tsx`)
7. Add visuals for the two statuses `TaskUpdate` introduces:
   - `in_progress` → spinning `Loader2` (with `animate-spin`), primary color (the one genuinely new, "actively working" state).
   - `pending` → hollow `Circle`, muted (rare; "not started").
   - Wire both into `statusVisual` and `statusBadgeCls`. Progress math (`done = counts.completed`) is unchanged.

### Frontend (`src/components/detail/AgentPanel.tsx`)
8. Drop the stray `console.log` debug lines (501-502) and the dead `if (event.name === "TaskCreate") break;` (500). The backend now suppresses `TaskUpdate` tool_use rows at the source; the existing `TaskCreate` guard becomes dead too and can go.

## Scope notes
- The persisted `tasks[]` stays an arrival-order log (its `TaskUpdate` entries will have empty subjects). This is fine because `Chat.tsx:372-376` explicitly does **not** render persisted tasks — the field has no current consumer — and the live TaskPanel is the merging surface. I'm keeping the backend minimal rather than adding a merge map for an unused field.
- No `TaskRecord` schema change needed: `status` is already a free-form `string` on both sides, so the new `in_progress`/`pending` values flow through without type edits.

## Verification
- `cargo test` (new + existing task tests).
- `cargo clippy` / `cargo fmt`.
- TypeScript typecheck/lint.
- Manual: run a turn that creates tasks then marks them in_progress → completed, and confirm the TaskPanel transitions (spinner → check) with progress bar advancing.