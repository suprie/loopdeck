#!/usr/bin/env bash
# Remove all git worktrees under .claude/worktrees and their local branches.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

git worktree list --porcelain | awk '/^worktree /{wt=$2} /^branch /{print wt, $2}' | while read -r wt branch; do
  [ "$wt" = "$(pwd)" ] && continue
  echo "Removing worktree: $wt ($branch)"
  git worktree remove --force "$wt"
  b="${branch#refs/heads/}"
  git branch -D "$b" 2>/dev/null || true
done

git worktree prune
