#!/usr/bin/env bash
# PreToolUse(Agent|Task): block write delegation onto a dirty shared tree.
# Read-only agents pass. Isolated agents pass: isolation "worktree"/"remote"
# gives the agent a fresh copy, and a session already inside a recorded
# worktree (.claude/worktrees or .agent-worktrees) owns that tree. Otherwise
# the tree the agent would write in — the hook cwd, never CLAUDE_PROJECT_DIR,
# which still names the main checkout after EnterWorktree — must be clean.
input=$(cat)
parsed=$(printf '%s' "$input" | node -e '
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", chunk => input += chunk);
process.stdin.on("end", () => {
  let j = {};
  try { j = JSON.parse(input) ?? {}; } catch {}
  const t = j.tool_input ?? {};
  process.stdout.write(
    [t.subagent_type ?? "", t.isolation ?? "", j.cwd ?? ""].join("\n"));
});
' 2>/dev/null)
subagent_type=$(printf '%s\n' "$parsed" | sed -n 1p)
isolation=$(printf '%s\n' "$parsed" | sed -n 2p)
hook_cwd=$(printf '%s\n' "$parsed" | sed -n 3p)
case "${subagent_type##*:}" in # strip plugin prefix (caveman:cavecrew-reviewer)
  cavecrew-investigator|cavecrew-reviewer|jet-verify|jet-ballot|read-only|claude-code-guide|Explore|Plan)
    exit 0 ;;
esac
case "$isolation" in worktree|remote) exit 0 ;; esac
tree="${hook_cwd:-$PWD}"
case "$tree" in
  */.claude/worktrees/*|*/.agent-worktrees/*) exit 0 ;;
esac
cd "$tree" 2>/dev/null || exit 0
if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
  echo "Dirty checkout: direct-tree write delegation blocked. Relaunch the agent with isolation \"worktree\", or work from a recorded worktree under .claude/worktrees/<name> (never a sibling jet-* folder), or commit only this task's explicitly owned paths first. Never use git add -A or include another task's changes." >&2
  exit 2
fi
exit 0
