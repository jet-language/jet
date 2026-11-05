#!/usr/bin/env bash
# Revert the uncommitted leftovers of failed lander commits in the dev worktree, path by path (only paths that
# differ from HEAD), then remove new files the failed patches created that HEAD does not track. The worktree is
# $JET_DEV_WORKTREE (Tools/agent/roots.sh).
set -u
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
cd "$JET_DEV_WORKTREE" || exit 2
mapfile -d '' paths < <(git diff --name-only -z HEAD)
mapfile -d '' added < <(git diff --name-only -z --diff-filter=A HEAD)
echo "changed ${#paths[@]} added ${#added[@]}"
[ ${#paths[@]} -eq 0 ] && exit 0
git reset -q -- "${paths[@]}"
for p in "${added[@]}"; do rm -f -- "$p"; done
mapfile -d '' tracked < <(git diff --name-only -z --diff-filter=MD HEAD)
[ ${#tracked[@]} -gt 0 ] && git checkout -q -- "${tracked[@]}"
# Ignored files the RustBridge patch created under its new home (not tracked at HEAD).
R=Jetpack/Integrations/Source/Foreign/RustBridge
if [ -d "$R" ] && [ -z "$(git ls-files -- "$R" | head -1)" ]; then rm -rf -- "$R"; echo "removed untracked $R"; fi
echo "left: $(git status --porcelain --untracked-files=no | wc -l)"
