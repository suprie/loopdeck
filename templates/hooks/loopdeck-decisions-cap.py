#!/usr/bin/env python3
"""LoopDeck PostToolUse hook — enforces the decisions.md live-entry cap.

templates/skills/loopdeck-memory/SKILL.md documents a 15-live-entry cap on
`.loopdeck/decisions.md` (overflow moves to `decisions-archive.md`), but that
convention is "enforced by you, the writer, not by a parser" — it only works
if every session remembers to archive on the write that crosses 15. This hook
makes it unconditional: after any Edit/Write/MultiEdit in a LoopDeck-tracked
project, if decisions.md has grown past the cap, the oldest entries are moved
to the archive automatically, no model turn required.

Matcher in settings.json: `"Edit|Write|MultiEdit"`. Deliberately does not
inspect the tool call's file path — decisions.md is small (capped), so
re-scanning it after every edit is cheap, and doing so unconditionally avoids
depending on the hook stdin JSON schema for tool_input shape.
"""
import json
import os
import re
import sys

LOOPDECK_DIR = ".loopdeck"
DECISIONS_PATH = os.path.join(LOOPDECK_DIR, "decisions.md")
ARCHIVE_PATH = os.path.join(LOOPDECK_DIR, "decisions-archive.md")
LIVE_CAP = 15
POINTER = (
    "_Older decisions archived to [decisions-archive.md](./decisions-archive.md)._"
)
HEADING_RE = re.compile(r"^## ", re.MULTILINE)


def split_entries(body: str):
    """Split decisions.md's body (after the title/pointer preamble) into a
    list of entry strings, each starting at its own '## ' heading."""
    starts = [m.start() for m in HEADING_RE.finditer(body)]
    if not starts:
        return []
    starts.append(len(body))
    return [body[starts[i] : starts[i + 1]] for i in range(len(starts) - 1)]


def main():
    if not os.path.isdir(LOOPDECK_DIR) or not os.path.isfile(DECISIONS_PATH):
        return 0

    with open(DECISIONS_PATH, "r", encoding="utf-8") as f:
        text = f.read()

    entries = split_entries(text)
    if len(entries) <= LIVE_CAP:
        return 0

    overflow_count = len(entries) - LIVE_CAP
    overflow, live = entries[:overflow_count], entries[overflow_count:]

    if os.path.isfile(ARCHIVE_PATH):
        with open(ARCHIVE_PATH, "a", encoding="utf-8") as f:
            f.write("".join(overflow))
    else:
        with open(ARCHIVE_PATH, "w", encoding="utf-8") as f:
            f.write("# Decisions Archive\n\n" + "".join(overflow))

    with open(DECISIONS_PATH, "w", encoding="utf-8") as f:
        f.write(f"# Decisions\n\n{POINTER}\n\n" + "".join(live))

    print(
        json.dumps(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": (
                        f"LoopDeck: auto-archived {overflow_count} decision(s) "
                        f"to decisions-archive.md ({LIVE_CAP} live entries kept)."
                    ),
                }
            }
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
