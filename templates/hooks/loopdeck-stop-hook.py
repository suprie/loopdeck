#!/usr/bin/env python3
"""LoopDeck Stop hook — only emits memory-update reminder when code changed.

Gated by .loopdeck/ directory existence (only fires in LoopDeck-tracked projects)
and .claude/.session-dirty (only fires when file changes occurred this session).
The shell fallback (loopdeck-memory-write.sh) is the sole consumer of the dirty file.
"""
import json
import os

LOOPDECK_DIR = ".loopdeck"
DIRTY_FILE = ".claude/.session-dirty"

# Gate: only fire in LoopDeck-tracked projects
if not os.path.isdir(LOOPDECK_DIR):
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "Stop",
            "additionalContext": "",
        }
    }))
    exit(0)

REMINDER = (
    "LOOPDECK MEMORY UPDATE REQUIRED:\n"
    "Before ending this session, update the project memory files:\n"
    "1. Read .loopdeck/decisions.md and append any architectural decisions made "
    "this session (format: ## YYYY-MM-DD — Title, then "
    "- **Status**: accepted|proposed|superseded, - **Context**: ..., "
    "- **Consequences**: ...).\n"
    "2. Read .loopdeck/loops.md and update the Current loop status, refresh "
    "Next Steps, and move any completed loops to History under ## History.\n"
    "3. If you completed a loop this session, move it to History as "
    "### YYYY-MM-DD — Title and set the next loop as Current.\n"
    "\n"
    "These files are displayed in LoopDeck UI tabs."
)

dirty = os.path.exists(DIRTY_FILE)
if dirty:
    # Don't delete the dirty file — the shell fallback hook also needs to see it.
    # The shell script (loopdeck-memory-write.sh) is the sole consumer.
    msg = REMINDER
else:
    msg = ""

print(json.dumps({
    "hookSpecificOutput": {
        "hookEventName": "Stop",
        "additionalContext": msg,
    }
}))
