#!/usr/bin/env bash
# Persistent builder worktree (.claude/worktrees/builder): build-heavy agent
# tasks reuse this one fixed path so cargo's warm cache carries across agents.
# Random per-agent worktree paths force a full cold rebuild every time —
# cargo fingerprints embed absolute paths (owner directive, 2026-08-07).
#
#   builder-sync.sh <claimant>   refresh to master HEAD and claim (exit 75 if busy)
#   builder-sync.sh --release    release the claim
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
builder="$repo/.claude/worktrees/builder"
lock="$repo/.claude/builder-claim"

if [ "${1:-}" = "--release" ]; then
  rm -f -- "$lock"
  echo "builder-sync: released"
  exit 0
fi

[ -d "$builder" ] || git -C "$repo" worktree add "$builder" -b builder
if [ -e "$lock" ]; then
  echo "builder-sync: busy — claimed by $(cat "$lock"). Serialize build agents; parallel builds contend anyway." >&2
  exit 75
fi
if [ -n "$(git -C "$builder" status --porcelain)" ]; then
  echo "builder-sync: builder worktree is dirty — integrate or hand off its work before reclaiming." >&2
  exit 65
fi
git -C "$builder" fetch -q . master
git -C "$builder" reset -q --hard FETCH_HEAD
printf '%s\n' "${1:-agent}" > "$lock"
echo "builder-sync: builder at $(git -C "$builder" rev-parse --short HEAD), claimed by ${1:-agent}"
