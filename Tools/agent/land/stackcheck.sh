#!/usr/bin/env bash
# stackcheck.sh patch... : do these patches apply in order on dev HEAD ($JET_DEV_WORKTREE)? (temporary index, no
# worktree change)
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
mkdir -p "$JET_RUN_ROOT"
cd "$JET_DEV_WORKTREE" || exit 2
GIT_INDEX_FILE=$(mktemp "$JET_RUN_ROOT/stackidx.XXXXXX"); export GIT_INDEX_FILE
err=$(mktemp "$JET_RUN_ROOT/stackcheck.XXXXXX")
trap 'rm -f "$GIT_INDEX_FILE" "$err"' EXIT
git read-tree HEAD
for p in "$@"; do
  if git apply --cached "$p" 2>"$err"; then echo "OK   $(basename "$p")"; else echo "FAIL $(basename "$p"): $(head -2 "$err" | tr '\n' ' ')"; fi
done
