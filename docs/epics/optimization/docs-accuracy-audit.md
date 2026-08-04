# Docs Accuracy Audit — Phase 1 Findings

Audit date: 2026-08-04
Scope: `docs/PRD.md` and `CLAUDE.md` vs the current source tree.
Source of truth: `src-tauri/src/` (32 top-level `.rs` files + `commands/` dir, 9
files), `src/` frontend tree, `git log`.

Each entry: the document claim → what the tree actually shows → `file:line`
evidence. Contradictions are numbered; the Phase 2 rewrites resolve every one.

---

## `docs/PRD.md` — contradictions

### D1. Status line: "30 Rust tests pass" (PRD.md:5)
- **Claim**: "The application builds, 30 Rust tests pass"
- **Actual**: 569 `#[test]` functions across 35 modules (`src-tauri/src/` and
  `src-tauri/src/commands/`).
- **Evidence**: `grep -rn "#[test]" src-tauri/src/ | wc -l` → 569. Largest
  modules: `epic.rs` 56, `conversation.rs` 46, `config.rs` 39, `permission.rs`
  38, `agents.rs` 38.

### D2. "Version 1 focuses exclusively on project discovery and memory initialization" (PRD.md:17)
- **Claim**: "Future versions may add AI agents, engineering loops, and
  automation. Version 1 focuses exclusively on project discovery and memory
  initialization."
- **Actual**: agent execution and engineering loops ship today.
- **Evidence**: `agents.rs` (Agent runtime, 1905 lines), `execution.rs`
  (`.loopdeck/execution.yaml` loop state), `epic.rs` (Epic/PRD/Phase hierarchy),
  `runplan.rs` (run plans), `claude_session.rs`, `codex_session.rs`.

### D3. Non-Goals section lists 9 items as current constraints (PRD.md:53-63)
The heading reads "Version 1 will NOT include:" with no historical marking.
Six of the nine shipped:

| Non-goal | Status | Evidence |
|---|---|---|
| Agent execution | **SHIPPED** | `src-tauri/src/agents.rs`, `multi_agent.rs`, `harness.rs`, `execution.rs`, `commands/agent.rs` (22 commands) |
| Claude Code integration | **SHIPPED** | `src-tauri/src/claude_session.rs` (2844 lines) |
| Codex integration | **SHIPPED** | `src-tauri/src/codex_session.rs` |
| Prompt generation | not shipped | no dedicated prompt-generation surface in the tree |
| Next Loop suggestions | **SHIPPED** | `run_queue.rs`, `commands/execution.rs:111` `promote_next_queued_loop`, `runplan.rs` |
| Activity tracking | **SHIPPED** | `src/hooks/useActivityEvents.tsx`, `src/components/activity/ActivityFeed.tsx` |
| Decision tracking | **SHIPPED** | `memory.rs` `parse_decisions` (:75), `commands/epics::get_decisions`, `src/components/decisions/DecisionsView.tsx` |
| Cloud synchronization | not shipped | no cloud/remote-sync code in tree |
| Team collaboration | not shipped | no collaboration code in tree |

### D4. "Only a single file is created in Version 1" + future memory files (PRD.md:169-177)
- **Claim**: repository structure is only `.loopdeck/project.yaml`; "Future
  versions may add: decisions.md, loops.md, activity.md, agents.md, context.md"
- **Actual**: `.loopdeck/` now carries `decisions.md`, `loops.md`,
  `execution.yaml`, `project.yaml`, plus `hooks/`, `sessions/`. The parsers and
  loop-execution state are shipped code.
- **Evidence**: `memory.rs` parses decisions/loops; `execution.rs` owns
  `execution.yaml`; `.loopdeck/` in this repo shows the full layout.

### D5. IPC command table: 10 commands (PRD.md:303-315)
- **Claim**: "Tauri IPC Commands (API Surface)" table lists 10 commands.
- **Actual**: 81 `#[tauri::command]` handlers are registered.
- **Evidence**: `src-tauri/src/lib.rs:157` `tauri::generate_handler![...]`
  registers 81 commands across `composer` (4), `project` (12), `epics` (10),
  `execution` (9), `run_queue` (9), `config_cmds` (11), `agent` (22),
  `multi_agent` (4).

### D6. Source File Map backend: single `commands.rs` (PRD.md:338-347)
- **Claim**: `commands.rs` is one file with "10 Tauri IPC handlers"; `lib.rs`
  is "state, 10 command registration".
- **Actual**: no `commands.rs`; handlers live in `src-tauri/src/commands/`
  (9 files); 81 commands registered.
- **Evidence**: `ls src-tauri/src/commands/` → `agent.rs`, `composer.rs`,
  `config_cmds.rs`, `epics.rs`, `execution.rs`, `mod.rs`, `project.rs`,
  `run_queue.rs`, `state.rs`. `lib.rs:157`.

### D7. Source File Map frontend (PRD.md:349-368)
- **Claim**: `App.css`; no `router.tsx`; `lib/` = 2 files, `store/` = 1,
  `hooks/` = 1, `components/` = 10 files.
- **Actual**: `styles.css` (no `App.css`); `router.tsx` present; `lib/` 7 files,
  `store/` 3, `hooks/` 4, `components/` 68 files in 14 subdirectories.
- **Evidence**: `ls src/`; `find src/components -name "*.tsx" | wc -l` → 68.

### D8. Test Coverage section: "30 Rust unit tests across 4 test modules" (PRD.md:373-384)
- **Claim**: `scanner` 11, `project` 10, `config` 6, `git` 3; total 30.
- **Actual**: `scanner` 15, `project` 11, `config` 39, `git` 12, and 31 more
  modules (569 total).
- **Evidence**: `grep -rn "#[test]" src-tauri/src/` per-file counts.

---

## `CLAUDE.md` — contradictions

### C1. Project Structure tree: frontend half stale (CLAUDE.md:13-21)
- **Claim**: `App.tsx / App.css`; omits `router.tsx`; `lib/tauri.ts` only;
  `store/appStore.ts` only; generic `hooks/` and `components/`.
- **Actual**: `styles.css` (not `App.css`); `router.tsx` present; `lib/` 7
  files (`tauri.ts`, `utils.ts`, `time.ts`, `markdown.ts`, `theme.tsx`,
  `attachments.ts`, `agentRosterClient.ts`); `store/` 3 files; `hooks/` 4
  files; `components/` 68 files in 14 subdirectories.
- **Evidence**: `ls src/lib src/store src/hooks`; `find src/components -name
  "*.tsx" | wc -l` → 68.

### C2. Project Structure tree: backend half stale (CLAUDE.md:23-33)
- **Claim**: backend is 8 files ending in single `commands.rs` with
  "(12 commands)"; omits the `commands/` directory.
- **Actual**: 32 top-level `.rs` files + `commands/` dir (9 files); 81
  commands. Missing from the tree: `agents.rs`, `claude_session.rs`,
  `codex_session.rs`, `epic.rs`, `execution.rs`, `runplan.rs`, `run_executor.rs`,
  `multi_agent.rs`, `harness.rs`, `secret_scan.rs`, etc.
- **Evidence**: `ls src-tauri/src/*.rs | wc -l` → 32; `ls src-tauri/src/commands/`.

### C3. Project Structure tree: skills location (CLAUDE.md:36)
- **Claim**: dev skills live in `.claude/skills/`.
- **Actual**: skills live in `.agents/skills/` (this repo has no `.claude/`
  dir at all).
- **Evidence**: `ls .agents/skills/` → 8 skills; `.claude/` absent.

### C4. Context Discipline file-size callouts (CLAUDE.md:93-95)
- **Claim**: "`commands.rs` ~30K tok, `claude_session.rs` ~24K,
  `conversation.rs` / `agents.rs` / `epic.rs` / `config.rs` each >12K"
- **Actual** (lines + bytes):
  - `commands.rs` — **file does not exist**; handlers moved to
    `commands/` (largest: `commands/agent.rs` 2221 lines, `commands/run_queue.rs`
    1991 lines).
  - `claude_session.rs` — 2844 lines, 132,209 bytes
  - `conversation.rs` — 2227 lines, 86,606 bytes
  - `agents.rs` — 1905 lines, 84,706 bytes
  - `epic.rs` — 2449 lines, 86,919 bytes
  - `config.rs` — 1480 lines, 55,790 bytes (crosses the 1500-line split
    threshold on the next growth)
  - Missed large files: `codex_session.rs`, `commands/agent.rs`,
    `commands/run_queue.rs`.
- **Evidence**: `wc -l` / `wc -c` on each file.

### C5. "V1 does NOT include agents, loops, cloud, or collaboration" (CLAUDE.md:89)
- **Claim**: V1 excludes agents, loops, cloud, collaboration.
- **Actual**: agents and loops ship (`agents.rs`, `execution.rs`, `epic.rs`,
  run queue). Cloud and collaboration do not ship.
- **Evidence**: `src-tauri/src/agents.rs`, `execution.rs`, `epic.rs`,
  `commands/run_queue.rs`, `runplan.rs`.

---

## Cross-links for shipped non-goals (Phase 2)

| Shipped non-goal | Actual shipping epic |
|---|---|
| Agent execution | `agent-full-access` (autonomous agent runtime + verify/ship) |
| Claude Code integration | `multi-model-agents` (`claude_session.rs`) |
| Codex integration | `multi-model-agents` (`codex_session.rs`) |
| Next Loop suggestions | `overnight-orchestration` (run queue, `promote_next_queued_loop`) |
| Activity tracking | `support-project-management` (activity/decisions/loops views) |
| Decision tracking | `support-project-management` (decisions parser + view) |

Not shipped, remain non-goals: Prompt generation, Cloud synchronization, Team
collaboration.
