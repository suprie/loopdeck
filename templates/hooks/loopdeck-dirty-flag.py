#!/usr/bin/env python3
"""LoopDeck PreToolUse hook — marks the session as dirty on file changes.

Creates `.claude/.session-dirty` when an Edit / Write / MultiEdit / NotebookEdit
fires inside a LoopDeck-tracked project. The Stop hook
(loopdeck-stop-hook.py) reads this flag to decide whether to emit the
memory-update nudge, and loopdeck-memory-write.sh consumes (deletes) it.

Without this creator, the Stop hook never sees a dirty session and the gating
that prevents Q&A-token waste is broken.

Matcher in settings.json: `"Edit|Write|MultiEdit|NotebookEdit"`.
"""
import json
import os
import sys

LOOPDECK_DIR = ".loopdeck"
DIRTY_FILE = ".claude/.session-dirty"


def main():
    # Only fire in LoopDeck-tracked projects.
    if not os.path.isdir(LOOPDECK_DIR):
        return 0

    # Touch the dirty flag. mtime update is enough; the Stop hooks only test
    # for existence. We don't delete on tool failure — a stale true-positive
    # (an edit was *attempted*) is fine; the Stop nudge is append-only.
    os.makedirs(os.path.dirname(DIRTY_FILE), exist_ok=True)
    open(DIRTY_FILE, "a").close()

    # Neutral passthrough — let the tool call proceed unchanged.
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "additionalContext": "",
        }
    }))
    return 0


if __name__ == "__main__":
    sys.exit(main())
