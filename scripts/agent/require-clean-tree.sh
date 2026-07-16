#!/usr/bin/env bash
# PreToolUse(Agent): block direct-tree write delegation when the checkout is
# dirty. Read-only agents pass. Write agents use a clean recorded worktree or
# wait until owned paths are committed; never sweep unrelated paths into one.
input=$(cat)
subagent_type=$(printf '%s' "$input" | node -e '
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => {
  try {
    const value = JSON.parse(input)?.tool_input?.subagent_type;
    if (typeof value === "string") process.stdout.write(value);
  } catch {}
});
' 2>/dev/null)
case "$subagent_type" in
  cavecrew-investigator|cavecrew-reviewer|jet-verify|jet-ballot|read-only|claude-code-guide|Explore|Plan|statusline-setup)
    exit 0 ;;
esac
cd "${CLAUDE_PROJECT_DIR:-.}" 2>/dev/null || exit 0
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
  echo "Dirty checkout: direct-tree write delegation blocked. Use a clean recorded worktree, or commit only this task's explicitly owned paths. Never use git add -A or include another task's changes. Retry from the isolated checkout." >&2
  exit 2
fi
exit 0
