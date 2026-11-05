#!/usr/bin/env bash
# suite.sh TAG TREE JETDIR SUITE [testkit args...]  (TESTKIT=path overrides the testkit tk-build.sh builds from TREE)
# Snapshot JETDIR (an s1 dir) to $SUITE_OUT/jet-TAG, run testkit SUITE from TREE, write $SUITE_OUT/SUITE-TAG.txt.
# SUITE_OUT defaults to $JET_SCRATCH_ROOT/lanes/Main/q/sg. Roots: Tools/agent/roots.sh.
# --jetpack: base/SHA/jetpack is passed when it exists, where SHA is JETDIR/COMMIT, else TREE/.chain-base
# (a hosted engine built from the same commit as the jet under test); an explicit --jetpack arg wins.
# publish-base.sh gives every published base its jetpack; a cases/all run without one prints WARN first and last.
set -u
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$here/../roots.sh"
tag=$1; tree=$2; jetdir=$3; suite=$4; shift 4
sg=${SUITE_OUT:-$JET_SCRATCH_ROOT/lanes/Main/q/sg}
B=$JET_BASE_ROOT
# Test scratch and TMPDIR live under the state root: a slow-fsync scratch disk turns durable-state tests into
# false 120 s timeouts (GoldC-3, 2026-10-31).
fs=$JET_STATE_ROOT/scratch/Main/sgfs
mkdir -p $sg/tmp $fs/tmp
sha=$(awk 1 "$jetdir/COMMIT" 2>/dev/null || awk 1 "$tree/.chain-base" 2>/dev/null)
jetpack=()
warn=""
case " $* " in
  *" --jetpack "*) echo "jetpack: explicit argument" ;;
  *) if [ -n "$sha" ] && [ -x "$B/$sha/jetpack" ]; then
       jetpack=(--jetpack "$B/$sha/jetpack"); echo "jetpack: ${jetpack[1]}"
     elif [ "$suite" = cases ] || [ "$suite" = all ]; then
       # Fall back to the newest published jetpack (a STAND-IN) so engine cases compare like for like with the
       # base run instead of erroring; a wave that changes Jetpack must still be proved with its own jetpack.
       standin=$(ls -td "$B"/*/ 2>/dev/null | while read -r d; do [ -x "$d/jetpack" ] && { readlink -f "$d/jetpack"; break; }; done)
       if [ -n "$standin" ]; then
         jetpack=(--jetpack "$standin")
         warn="WARN: STAND-IN JETPACK for ${sha:-unknown commit}: no $B/${sha:-?}/jetpack, using $standin; Jetpack source changes in this tree are NOT exercised"
       else
         warn="WARN: NO JETPACK for ${sha:-unknown commit} (no $B/${sha:-?}/jetpack): ~156 jetpack cases rows will ERROR 'jetpack binary required'; build it with: SLOT_SMALL=0 Tools/agent/chain/build-slot.sh bash Tools/agent/land/publish-base.sh --jetpack-run ${sha:-COMMIT}"
       fi
       echo "$warn"; echo "$warn" >&2
     else echo "jetpack: none (suite $suite does not use it)"; fi ;;
esac
# The ui and cases runs of one tag start together and share this snapshot: copy it once, under a lock.
# The copy lives on the root filesystem ($fs/jet-TAG); q/sg/jet-TAG is a symlink to it, so callers keep one path.
if [ "$jetdir" != "$sg/jet-$tag" ]; then
  ( flock 9
    if ! cmp -s "$jetdir/jet" "$fs/jet-$tag/jet" 2>/dev/null; then
      rm -rf "$fs/jet-$tag" "$sg/jet-$tag" && cp -r "$jetdir" "$fs/jet-$tag" && ln -sfn "$fs/jet-$tag" "$sg/jet-$tag" || exit 1
    fi
    [ -e "$sg/jet-$tag/jet" ] || ln -sfn "$fs/jet-$tag" "$sg/jet-$tag"
  ) 9>"$sg/jet-$tag.lock" || exit 1
fi
# `jet` finds its engine beside itself before PATH: give a snapshot without one the jetpack this run passes,
# or engine-dispatch cases reach a stale PATH jetpack and fail E1227 (wave 11, 2026-10-31).
if [ "${#jetpack[@]}" -gt 0 ] && [ ! -e "$sg/jet-$tag/jetpack" ]; then ln -s "${jetpack[1]}" "$sg/jet-$tag/jetpack"; fi
# The testkit is built from TREE itself with the base compiler (cached by a hash of Tools/testkit), so a stale
# prebuilt testkit never misreads a newer tree. TESTKIT=path still overrides.
bdir=$B/$sha; [ -x "$bdir/jet" ] || bdir=$B/latest
if [ -z "${TESTKIT:-}" ]; then
  TESTKIT=$("$here/tk-build.sh" "$tree" "$bdir") || { echo "testkit build failed for $tree"; exit 1; }
fi
echo "testkit: $TESTKIT"
cd "$tree" || exit 1
ID=$(awk 1 "$B/latest/IDENTITY")
rm -rf "$fs/sc-$suite-$tag"
# --jobs: the kit inside a build-slot scope must not size its pool from the host's MemAvailable
# (2026-11-01: four shards of that took the machine into swap and the desktop down). 3 GiB per entry: at 2 GiB
# (7 jobs in 16G) the cases suite hit the slot's memory-pressure kill.
slot_mem=${SLOT_MEMORY_MAX:-16G}; slot_mem=${slot_mem%G}
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=$PWD TMPDIR=$fs/tmp timeout 6800 "$tree/Tools/agent/jet-env" \
  "$TESTKIT" "$suite" --jet "$sg/jet-$tag/jet" "${jetpack[@]}" --jobs "${SUITE_JOBS:-$((slot_mem / 3))}" "$@" \
  --scratch "$fs/sc-$suite-$tag" > "$sg/$suite-$tag.txt" 2>&1
rc=$?
echo "rc=$rc $(awk 'END{print}' "$sg/$suite-$tag.txt" | cut -c1-240)"
[ -z "$warn" ] || echo "$warn"
