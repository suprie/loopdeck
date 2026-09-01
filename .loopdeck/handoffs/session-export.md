---
artifact: session-export
author_role: business-analyst
phase: handoff-spike/two-agent-run
type: plan
created: 2026-09-01
summary: One-click export of the currently selected session transcript to a Markdown file saved next to the project.
cites: []
---

# Session Export — Handoff Artifact

## Summary

Users sometimes need a durable, shareable record of an agent session (for
review, standup notes, or attaching to a bug). Today that means copying from
the UI or digging for raw session files. This feature adds a single-click
"Export Transcript" action that writes the selected session's transcript as a
readable Markdown file next to the project. Fictional spike feature; will not
be implemented.

## Requirements

- **R1**: The session detail view shall show an "Export Transcript" button, enabled only when a session is currently selected.
- **R2**: Clicking the button shall export the full transcript of the selected session in chronological order, with speaker attribution for each entry (user vs agent).
- **R3**: The export shall include tool calls and their results as human-readable one-line summaries, not raw JSON payloads.
- **R4**: The file shall be written to `<project-root>/.loopdeck/exports/session-<session-id>.md`, creating the directory if missing.
- **R5**: The action shall be one-click end-to-end: no intermediate dialog, no options to pick; the filename is derived, never typed by the user.
- **R6**: The exported file shall begin with a metadata header block: session id, agent name (Claude Code or Codex), session start time, and message count.
- **R7**: The write shall be atomic — on any failure (unwritable path, vanished session) the user sees an error notification and no partial file is left on disk.
- **R8**: On success the user sees a notification containing the absolute file path; re-exporting the same session overwrites its existing file.

## Constraints

- **C1**: Entirely local and offline — the export flow shall make no network calls and depend on no cloud service.
- **C2**: No new third-party dependencies; the implementation shall use the existing typed IPC wrappers and Tauri filesystem APIs only.

## Non-Goals

- No export formats other than Markdown (no PDF, HTML, or JSON).
- No batch export of multiple sessions or whole projects.
- No editing, redaction, or filtering of transcript content before export — the file mirrors the session as recorded.

## Open Questions

- **Q1**: Does "saved next to the project" mean inside the repo (`.loopdeck/exports/`, as R4 assumes) or a sibling directory outside the repo working tree?
- **Q2**: Are one-line tool-call summaries (R3) sufficient for debugging use cases, or do some workflows need the full tool payloads?
