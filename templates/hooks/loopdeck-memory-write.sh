#!/usr/bin/env bash
# LoopDeck memory auto-write helper (Approach B — shell fallback).
# Called by the Stop hook to append a basic session summary to
# .loopdeck/ files when the AI hasn't written them yet.
#
# Gated by .loopdeck/ directory (only active in LoopDeck-tracked projects)
# and .claude/.session-dirty (only fires when file changes occurred).
#
# This is a mechanical fallback — the primary mechanism is
# Approach A (prompt-based via loopdeck-stop-hook.py), which gives richer output.
# This script ensures the files always exist and have at minimum
# a timestamp entry.

set -euo pipefail

# Gate: only fire in LoopDeck-tracked projects
LOOPDECK_DIR=".loopdeck"
if [ ! -d "$LOOPDECK_DIR" ]; then
  exit 0
fi

# Only write heartbeat if session actually changed files
DIRTY_FILE=".claude/.session-dirty"
if [ ! -f "$DIRTY_FILE" ]; then
  exit 0
fi
rm -f "$DIRTY_FILE"

DECISIONS_FILE="$LOOPDECK_DIR/decisions.md"
LOOPS_FILE="$LOOPDECK_DIR/loops.md"
TODAY=$(date +%Y-%m-%d)

# Initialize decisions.md if absent
if [ ! -f "$DECISIONS_FILE" ]; then
  printf "# Decisions\n\n" > "$DECISIONS_FILE"
fi

# Initialize loops.md if absent
if [ ! -f "$LOOPS_FILE" ]; then
  cat > "$LOOPS_FILE" <<'EOF'
# Loops

## Current

_No active loop._

## Next Steps

## History

EOF
fi

# Append a session heartbeat to decisions.md (minimal — AI should replace with detail)
# Only adds a heartbeat if there isn't already an entry for today
if ! grep -q "^## $TODAY" "$DECISIONS_FILE"; then
  {
    echo ""
    echo "## $TODAY — Session heartbeat"
    echo "- **Status**: proposed"
    echo "- **Context**: AI session active on LoopDeck development."
    echo ""
  } >> "$DECISIONS_FILE"
fi

echo "LoopDeck memory files checked: $TODAY"
