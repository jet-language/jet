#!/usr/bin/env bash
# PreToolUse(Agent): block delegation to write-capable sub-agents while the
# tree is dirty — a sub-agent `git restore` has wiped uncommitted parent work.
# Read-only agent types pass through.
input=$(cat)
case "$input" in
  *cavecrew-investigator*|*cavecrew-reviewer*|*claude-code-guide*|*'"Explore"'*|*'"Plan"'*|*statusline-setup*)
    exit 0 ;;
esac
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
  echo "Dirty tree — checkpoint before delegating (sub-agent git-restore can wipe uncommitted work): git add -A && git commit -m 'wip: checkpoint before delegation' — then retry the Agent call." >&2
  exit 2
fi
exit 0
