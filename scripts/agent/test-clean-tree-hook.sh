#!/usr/bin/env bash
# Self-check for require-clean-tree.sh. Run: bash scripts/agent/test-clean-tree-hook.sh
set -u
s="$(cd "$(dirname "$0")" && pwd)/require-clean-tree.sh"
dirty=$(mktemp -d)
trap 'rm -rf "$dirty"' EXIT
git -C "$dirty" init -q
touch "$dirty/untracked"
mkdir -p "$dirty/.claude/worktrees/wt"
fail=0
run() { # name payload want
  printf '%s' "$2" | bash "$s" >/dev/null 2>&1
  got=$?
  [ "$got" -eq "$3" ] || { echo "FAIL: $1 (want $3, got $got)"; fail=1; return 1; }
}
run "read-only agent on dirty tree passes" "{\"cwd\":\"$dirty\",\"tool_input\":{\"subagent_type\":\"Explore\"}}" 0
run "write agent on dirty tree blocked" "{\"cwd\":\"$dirty\",\"tool_input\":{\"subagent_type\":\"general-purpose\"}}" 2
run "write agent in recorded worktree passes" "{\"cwd\":\"$dirty/.claude/worktrees/wt\",\"tool_input\":{\"subagent_type\":\"general-purpose\"}}" 0
run "isolation=worktree on dirty tree passes" "{\"cwd\":\"$dirty\",\"tool_input\":{\"subagent_type\":\"general-purpose\",\"isolation\":\"worktree\"}}" 0
run "plugin-prefixed read-only passes" "{\"cwd\":\"$dirty\",\"tool_input\":{\"subagent_type\":\"caveman:cavecrew-reviewer\"}}" 0
(cd "$dirty/.claude/worktrees/wt" && run "garbage input never blocks an isolated pwd" "not json" 0) || fail=1
[ "$fail" -eq 0 ] && echo "require-clean-tree self-check: all pass"
exit "$fail"
