#!/usr/bin/env bash
# integ-land.sh BATCH SUB... : lands the named submissions of a passed integration batch on dev, one commit each, in
# the order given (the order integrate.sh applied them). Refuses unless integ/BATCH's census and ratchets passed, each
# SUB's status is OK, and origin/dev is still the batch's BASE (any other movement means the composed tree was not
# the one tested: rerun integrate.sh). Commits in the dev worktree stage exactly each patch's files; the author's
# lane name goes in a Lane: trailer. Pushes once, then appends the shas to batch-queue.txt.
# The original of gauntlet tool entry integ-land4; IntegLand4.jet beside it is its Jet twin. The batches live in
# $JET_SCRATCH_ROOT/lanes/Main/integ, the dev worktree is $JET_DEV_WORKTREE, and treeonly.mjs, batch-queue.txt and
# landed-subs.txt are Main's, in $JET_STATE_ROOT/scratch/Main (Tools/agent/roots.sh).
set -u
BATCH=${1:?batch}; shift
. "$(dirname "$0")/../roots.sh"
M=$JET_SCRATCH_ROOT/lanes/Main
W=$JET_DEV_WORKTREE
G=$JET_STATE_ROOT/scratch/Main
I=$M/integ/$BATCH
grep -q '^Census rc=0 ' "$I/REPORT.md" || { echo "integ-land: batch $BATCH did not pass its census/ratchets"; exit 1; }
cd "$W" || exit 2
[ -z "$(git status --porcelain=v1 -uno)" ] || { echo "integ-land: dev worktree has changes"; exit 1; }
git fetch -q origin
base=$(cat "$I/BASE")
# origin/dev may have moved past BASE only by changes that are not build inputs and share no file with this batch:
# the net diff BASE..origin/dev may touch no Compiler/, Core/, Jetpack/ or Tools/ file and none of the batch files.
moved=$(git diff --name-only "$base" origin/dev)
bad=$(printf "%s\n" "$moved" | while read -r f; do [ -n "$f" ] || continue; case "$f" in Compiler/*|Core/*|Jetpack/*|Tools/*) echo "$f";; *) cat "$I"/*.files | grep -qx "$f" && echo "$f";; esac; done)
[ -z "$bad" ] || { echo "integ-land: origin/dev moved from $base in build inputs or batch files: $(echo $bad | cut -c1-300); rerun integrate.sh"; exit 1; }
git merge -q --ff-only origin/dev || { echo "integ-land: cannot fast-forward the worktree to origin/dev"; exit 1; }
shas=()
for s in "$@"; do
  s=${s%/}; n=$(basename "$s")
  [ "$(cat "$I/$n.status" 2>/dev/null)" = OK ] || { echo "integ-land: $n is not OK in $BATCH; stopping before it"; break; }
  P="$s/patch"; [ -s "$I/$n.rebased" ] && P="$I/$n.rebased"
  # An empty .rebased means the integ repo lost the commit (b6d: gc race); the raw patch is not what was tested.
  [ -e "$I/$n.rebased" ] && [ ! -s "$I/$n.rebased" ] && { echo "integ-land: $I/$n.rebased is empty; rebuild it (rebuild-rebased.sh) before landing"; exit 1; }
  git apply -p1 --index "$P" || { echo "integ-land: $n does not apply on $(git rev-parse --short HEAD)"; exit 1; }
  # Every path the patch staged, rename sources included (numstat names only the rename target: b6d left 12 behind).
  git diff --cached --name-only --no-renames > "$I/$n.landfiles"
  lane=${n%-*}
  { cat "$s/msg"; printf '\nLane: %s\nIntegrated: %s on %s\n' "$lane" "$BATCH" "$base"; } > "$I/$n.commitmsg"
  git commit -q -F "$I/$n.commitmsg" -- $(cat "$I/$n.landfiles") || { echo "integ-land: commit of $n failed"; exit 1; }
  shas+=("$(git rev-parse --short HEAD) $n")
done
[ ${#shas[@]} -gt 0 ] || exit 1
# The generated tables integ-apply regenerated from the composed sources land last, only when EVERY SUB the batch
# applied landed (b4b: landing a subset with the full regen put SchedLead-6 rows on dev without their source).
napplied=$(ls "$I"/*.files 2>/dev/null | awk "!/\/regen\.files$/" | wc -l)
if [ -s "$I/regen.patch" ]; then
  # INTEG_LANDED_BEFORE=k: k SUBs of this batch already landed in an earlier, interrupted run.
  if [ $(( ${#shas[@]} + ${INTEG_LANDED_BEFORE:-0} )) -eq "$napplied" ]; then
    git apply -p1 --index "$I/regen.patch" && git diff --cached --name-only --no-renames > "$I/regen.files" && printf "integration: regenerate the generated tables for %s\n\nIntegrated: %s on %s\n" "$BATCH" "$BATCH" "$base" > "$I/regen.commitmsg" && git commit -q -F "$I/regen.commitmsg" -- $(cat "$I/regen.files") && shas+=("$(git rev-parse --short HEAD) regen") || { echo "integ-land: regen commit failed"; exit 1; }
  else echo "integ-land: not every SUB landed, so regen.patch was not applied; rerun the generators"; exit 1; fi
fi
# The landed tree must be the tested composed tree, byte for byte, before anything is pushed.
[ -z "$(git status --porcelain=v1 -uno)" ] || { echo "integ-land: staged leftovers after landing: $(git status --porcelain=v1 -uno | head -n5 | tr '\n' ' ')"; exit 1; }
if [ ${#shas[@]} -gt 0 ] && [ "$(( ${#shas[@]} + ${INTEG_LANDED_BEFORE:-0} ))" -ge "$napplied" ]; then
  drift=$(git -c core.quotepath=off ls-files | while read -r f; do [ -L "$f" ] && continue; [ -e "$I/tree/$f" ] || { [ -e "$f" ] && echo "NOTINTREE $f"; continue; }; cmp -s "$f" "$I/tree/$f" || echo "DIFF $f"; done; node "$G/treeonly.mjs" "$I/tree" . | awk '!/ only-in-tree$/ && NF {print "ONLYINTREE " $0}')
  [ -z "$drift" ] || { echo "integ-land: landed tree differs from $I/tree: $(echo "$drift" | head -n8 | tr '\n' ' ')"; exit 1; }
fi
git fetch -q origin && git merge-base --is-ancestor origin/dev HEAD && git push -q origin HEAD:dev || { echo "integ-land: push failed"; exit 1; }
for x in "${shas[@]}"; do echo "${x%% *}" >> "$G/batch-queue.txt"; [ "${x#* }" = regen ] || echo "${x#* }" >> "$G/landed-subs.txt"; echo "LANDED $x"; done
