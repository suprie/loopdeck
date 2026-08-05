#!/usr/bin/env python3
"""Selasar Stop hook — append-only memory nudge (lean, no re-reads).

Design rules (learned the hard way — see decision 2026-07-19 "Stop hook
re-injection caused runaway token usage"):

  1. NEVER instruct the model to *read* loops.md / decisions.md. Those files
     can be large; forcing a re-read every turn is what burned ~30M tokens/hr.
     This nudge is ~3 lines and asks for an APPEND only.
  2. Gate on `.loopdeck/` (Selasar-tracked project) AND `.claude/.session-dirty`
     (file changes happened this turn). Without the dirty flag the hook is a
     no-op, so pure Q&A and read-only turns cost zero.
  3. The dirty flag is created by loopdeck-dirty-flag.py (PreToolUse on
     Edit/Write/MultiEdit) and consumed by loopdeck-memory-write.sh.
"""
import json
import os

LOOPDECK_DIR = ".loopdeck"
DIRTY_FILE = ".claude/.session-dirty"

# Short, append-only reminder. Deliberately does NOT say "read loops.md".
REMINDER = (
    "Selasar: if you made an architectural decision or changed the active "
    "loop this turn, APPEND a one-paragraph entry to "
    ".loopdeck/decisions.md (## YYYY-MM-DD — Title) or update the Current "
    "section of .loopdeck/loops.md. Skip if this turn was read-only or Q&A."
)

# Only fire in Selasar-tracked projects that changed files this turn.
if not os.path.isdir(LOOPDECK_DIR) or not os.path.exists(DIRTY_FILE):
    msg = ""
else:
    msg = REMINDER

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "Stop",
        "additionalContext": msg,
    }
}))
