---
artifact: session-export-em-plan
author_role: engineering-manager
phase: handoff-spike/two-agent-run
type: plan
created: 2026-09-01
summary: Three loops deliver one-click session transcript export — Rust renderer/writer, frontend button + toasts, hardening — grounded in session-export.md.
cites: [.loopdeck/handoffs/session-export.md]
---

## Summary

Three sequential loops: (1) Rust export command with Markdown renderer and
atomic writer, (2) frontend button + typed IPC + notifications, (3)
hardening and end-to-end verification. Delivers R1–R8 under C1–C2.

## Loops

### Loop 1 — Backend: renderer + atomic writer (src-tauri/)
- New command `export_session_transcript(project_id, session_id) ->
  Result<String>` in a new `src-tauri/src/commands/export.rs` (avoids
  growing the ~2.2k-line agent.rs), registered in `lib.rs`; returns the
  absolute path (R8).
- Renderer uses existing accessors in `claude_session.rs`
  / `codex_session.rs`; emits metadata header — session id, agent name,
  start time, message count (R6), chronological entries with speaker
  attribution user vs agent (R2), one-line tool-call/result summaries, not
  raw JSON (R3).
- Writer creates `<project-root>/.loopdeck/exports/` if missing (R4), writes
  a temp file in that dir, then renames to `session-<session-id>.md` —
  atomic, overwrite-safe re-export (R7, R8). Pure `std::fs`, no network
  (C1), no new deps (C2).
- Verify: `cargo test` — renderer (header, ordering, attribution, tool
  one-liners) and writer (dir creation, failure leaves no partial file,
  overwrite).

### Loop 2 — Frontend: button, IPC, notifications (src/)
- "Export Transcript" button in `src/components/agent/AgentRunner.tsx`,
  enabled only when a session is selected (R1); one click fires the
  command — no dialog, no options, derived filename (R5).
- Typed wrapper in `src/lib/tauri.ts` (C2 — no raw `invoke()`).
- Success: sonner toast with the absolute path (R8), reusing
  `src/components/ui/sonner.tsx`. Failure: error toast, no file (R7).
- Verify: `npx tsc --noEmit`; manual export of one Claude and one Codex
  session; button disabled with no selection.

### Loop 3 — Hardening + end-to-end verification
- R7 failure modes: unwritable path, vanished/invalid session id → clean
  IPC error toast, temp file removed.
- Cross-check R6 header against the session record; message count matches.
- Full gate: `cargo test`, `cargo clippy`, `npx tsc --noEmit`, manual
  export for both agents; no network (C1).

## Risks

- `claude_session.rs` (~2.8k lines) and `codex_session.rs` expose different
  transcript shapes → normalize behind one internal enum in export.rs; no
  adapter refactors.
- Windows rename-over-existing fails → temp-file + remove-then-rename so R8
  re-export works cross-platform while staying atomic in the common case.
- Live sessions export a snapshot; acceptable — file mirrors the session as
  recorded (Non-Goals).
- Tool payload variety may resist one-line summarization (R3) → cap length,
  fall back to tool name + status.

## Open question resolutions

- Q1: Inside the repo at `.loopdeck/exports/` — R4's path is normative and
  matches the existing `.loopdeck/` convention.
- Q2: One-line summaries stand (R3); full-payload export is excluded by
  Non-Goals. Revisit via a new artifact if needed.

## Handoff citations

- .loopdeck/handoffs/session-export.md#Summary — framed the loops.
- .loopdeck/handoffs/session-export.md#Requirements — R1–R8 drive scope.
- .loopdeck/handoffs/session-export.md#R1 — Loop 2.
- .loopdeck/handoffs/session-export.md#R2 — Loop 1.
- .loopdeck/handoffs/session-export.md#R3 — Loop 1.
- .loopdeck/handoffs/session-export.md#R4 — Loop 1.
- .loopdeck/handoffs/session-export.md#R5 — Loop 2.
- .loopdeck/handoffs/session-export.md#R6 — Loops 1, 3.
- .loopdeck/handoffs/session-export.md#R7 — Loops 1–3.
- .loopdeck/handoffs/session-export.md#R8 — Loops 1–2.
- .loopdeck/handoffs/session-export.md#Constraints — bound choices.
- .loopdeck/handoffs/session-export.md#C1 — Loop 3.
- .loopdeck/handoffs/session-export.md#C2 — Loops 1–2.
- .loopdeck/handoffs/session-export.md#Non-Goals — scope fence.
- .loopdeck/handoffs/session-export.md#Open Questions — resolved above.
- .loopdeck/handoffs/session-export.md#Q1 — resolved: inside repo per R4.
- .loopdeck/handoffs/session-export.md#Q2 — resolved: one-liners (R3).
