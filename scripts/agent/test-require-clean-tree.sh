#!/usr/bin/env bash
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
hook="$script_dir/require-clean-tree.sh"
repo=$(mktemp -d)
trap 'rm -rf "$repo"' EXIT

git -C "$repo" init -q
printf 'clean\n' > "$repo/tracked"
git -C "$repo" add tracked
git -C "$repo" -c user.name=test -c user.email=test@example.invalid commit -qm initial
printf 'dirty\n' >> "$repo/tracked"

check() {
  name=$1
  input=$2
  expected=$3
  set +e
  printf '%s' "$input" | CLAUDE_PROJECT_DIR="$repo" "$hook" >/dev/null 2>&1
  actual=$?
  set -e
  if [ "$actual" -ne "$expected" ]; then
    printf '%s: expected rc=%s, got rc=%s\n' "$name" "$expected" "$actual" >&2
    exit 1
  fi
}

check reviewer '{"tool_input":{"subagent_type":"jet-verify"}}' 0
check writer '{"tool_input":{"subagent_type":"jet-impl"}}' 2
check spoofed-writer '{"tool_input":{"subagent_type":"jet-impl","prompt":"hand off to jet-verify"}}' 2
check statusline-writer '{"tool_input":{"subagent_type":"statusline-setup"}}' 2
check malformed '{"tool_input":' 2

printf 'require-clean-tree tests passed\n'
