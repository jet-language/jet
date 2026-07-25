#!/usr/bin/env bash
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
hook="$script_dir/check-worktree-layout.sh"
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

repo="$root/jet"
leak="$root/jet-bd-leak"
mkdir -p "$repo" "$leak"
git -C "$repo" init -q
printf 'ok\n' >"$repo/tracked"
git -C "$repo" add tracked
git -C "$repo" -c user.name=test -c user.email=test@example.invalid commit -qm initial

set +e
out=$(JET_REPO_ROOT="$repo" "$hook" --strict 2>&1)
rc=$?
set -e
if [ "$rc" -ne 2 ]; then
  printf 'strict+sibling: expected rc=2, got %s\n%s\n' "$rc" "$out" >&2
  exit 1
fi
printf '%s\n' "$out" | grep -q 'jet-bd-leak' || {
  printf 'strict+sibling: expected leak path in output\n%s\n' "$out" >&2
  exit 1
}

rm -rf "$leak"
set +e
out=$(JET_REPO_ROOT="$repo" "$hook" --strict 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  printf 'strict+clean: expected rc=0, got %s\n%s\n' "$rc" "$out" >&2
  exit 1
fi

# Outside worktree path must fail strict mode.
outside="$root/outside-wt"
git -C "$repo" worktree add "$outside" -b tmp-outside
set +e
out=$(JET_REPO_ROOT="$repo" "$hook" --strict 2>&1)
rc=$?
set -e
if [ "$rc" -ne 2 ]; then
  printf 'strict+outside-wt: expected rc=2, got %s\n%s\n' "$rc" "$out" >&2
  exit 1
fi
git -C "$repo" worktree remove -f "$outside"
git -C "$repo" branch -D tmp-outside >/dev/null

# In-repo worktree path must pass.
inside="$repo/.claude/worktrees/ok-wt"
mkdir -p "$repo/.claude/worktrees"
git -C "$repo" worktree add "$inside" -b tmp-inside
set +e
out=$(JET_REPO_ROOT="$repo" "$hook" --strict 2>&1)
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  printf 'strict+inside-wt: expected rc=0, got %s\n%s\n' "$rc" "$out" >&2
  exit 1
fi
git -C "$repo" worktree remove -f "$inside"
git -C "$repo" branch -D tmp-inside >/dev/null

# Live checkout warn mode must succeed.
set +e
"$hook" >/dev/null
rc=$?
set -e
if [ "$rc" -ne 0 ]; then
  printf 'live warn mode: expected rc=0, got %s\n' "$rc" >&2
  exit 1
fi

printf 'check-worktree-layout tests passed\n'
