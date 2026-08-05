#!/usr/bin/env python3
"""
Selasar orchestrator-start hook — PreToolUse on Skill(loopdeck:orchestrator).
Writes .loopdeck/current-loop.md with a single task line (max 100 chars)
before the orchestrator delegates to agents.

Gated by .loopdeck/ directory (only fires in Selasar-tracked projects).
"""
import json
import os
import sys

MAX_TASK_LENGTH = 100


def main():
    # Read tool call info from stdin
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, IOError):
        pass_through()
        return 0

    tool_name = data.get("tool_name", "")
    tool_input = data.get("tool_input", {})
    skill_name = tool_input.get("skill", "")

    # Only fire for loopdeck:orchestrator
    if tool_name != "Skill" or skill_name != "loopdeck:orchestrator":
        pass_through()
        return 0

    # Gate: only fire in Selasar-tracked projects
    loopdeck_dir = ".loopdeck"
    if not os.path.isdir(loopdeck_dir):
        pass_through()
        return 0

    # Get orchestrator arguments and derive the task line
    args = tool_input.get("args", "").strip()

    if args:
        task = args
    else:
        task = "Orchestrating feature implementation"

    # Truncate to max length
    if len(task) > MAX_TASK_LENGTH:
        task = task[: MAX_TASK_LENGTH - 1] + "…"

    # Write current-loop.md — single task, replaced each invocation
    loopdeck_path = os.path.join(loopdeck_dir, "current-loop.md")
    os.makedirs(loopdeck_dir, exist_ok=True)

    with open(loopdeck_path, "w") as f:
        f.write(f"# Current Loop\n{task}\n")

    additional_context = f"Selasar: .loopdeck/current-loop.md → {task}"

    print(
        json.dumps({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": additional_context,
            }
        })
    )
    return 0


def pass_through():
    """Output neutral hook response — let the tool call proceed unchanged."""
    print(
        json.dumps({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "additionalContext": "",
            }
        })
    )


if __name__ == "__main__":
    sys.exit(main())
