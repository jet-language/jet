#!/usr/bin/env bash
# Warn (or fail with --strict) when Jet worktrees or checkouts leak beside the
# main clone. Canonical paths: <repo>/.claude/worktrees/* and
# <repo>/.agent-worktrees/*. See AGENTS.md § Ownership and worktrees.
set -eu

strict=0
if [ "${1:-}" = "--strict" ]; then
  strict=1
fi

if [ -n "${JET_REPO_ROOT:-}" ]; then
  repo=$(CDPATH= cd -- "$JET_REPO_ROOT" && pwd)
else
  repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
fi
parent=$(dirname -- "$repo")
base=$(basename -- "$repo")
problems=0

say() {
  printf '%s\n' "$*"
}

fail() {
  say "WORKTREE LAYOUT: $*"
  problems=$((problems + 1))
}

# Sibling jet* dirs/symlinks next to the main clone (except the clone itself).
if [ -d "$parent" ]; then
  shopt -s nullglob
  for path in "$parent"/jet "$parent"/Jet "$parent"/jet-* "$parent"/Jet-*; do
    [ -e "$path" ] || continue
    name=$(basename -- "$path")
    if [ "$name" = "$base" ]; then
      continue
    fi
    if [ -L "$path" ]; then
      fail "sibling symlink $path — remove it; use only $repo"
    else
      fail "sibling path $path — move worktree under $repo/.claude/worktrees/<name> or delete"
    fi
  done
  shopt -u nullglob
fi

# Registered git worktrees must be the main repo or an in-repo worktree dir.
if git -C "$repo" rev-parse --git-dir >/dev/null 2>&1; then
  while IFS= read -r line; do
    case "$line" in
      worktree\ *)
        wt=${line#worktree }
        if [ "$wt" = "$repo" ]; then
          continue
        fi
        case "$wt" in
          "$repo"/.claude/worktrees/*|"$repo"/.agent-worktrees/*)
            continue
            ;;
          *)
            fail "registered worktree outside in-repo dirs: $wt"
            ;;
        esac
        ;;
    esac
  done < <(git -C "$repo" worktree list --porcelain 2>/dev/null || true)
fi

if [ "$problems" -eq 0 ]; then
  say "WORKTREE LAYOUT: ok (only $repo at top level; worktrees under .claude/worktrees or .agent-worktrees)"
  exit 0
fi

say "WORKTREE LAYOUT: $problems problem(s). Fix: git worktree move <bad> $repo/.claude/worktrees/<short-name>"
say "See AGENTS.md § Ownership and worktrees."
if [ "$strict" -eq 1 ]; then
  exit 2
fi
exit 0
