#!/usr/bin/env bash
# PreToolUse(Agent): block direct-tree write delegation when the checkout is
# dirty. Read-only agents pass. Write agents use a clean recorded worktree or
# wait until owned paths are committed; never sweep unrelated paths into one.
input=$(cat)
case "$input" in
  *cavecrew-investigator*|*cavecrew-reviewer*|*jet-verify*|*jet-ballot*|*read-only*|*claude-code-guide*|*'"Explore"'*|*'"Plan"'*|*statusline-setup*)
    exit 0 ;;
esac
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
  echo "Dirty checkout: direct-tree write delegation blocked. Use a clean recorded worktree, or commit only this task's explicitly owned paths. Never use git add -A or include another task's changes. Retry from the isolated checkout." >&2
  exit 2
fi
exit 0
