#!/usr/bin/env bash
# suite.sh TAG TREE JETDIR PROFILE [jet test settings...]
# Snapshot JETDIR (an s1 dir) to $SUITE_OUT/jet-TAG, run `jet test Compiler --profile PROFILE` from TREE with
# the snapshot as `--compiler`, write $SUITE_OUT/PROFILE-TAG.txt. PROFILE is a Compiler/package.jet test
# profile (golden, ui, cases, corpora for all three, ...); the settings are `jet test` flags such as
# --filter=X, --cases=FILE, --optimize=none,full or --results=FILE.
# The running jet is SUITE_RUNNER, else the snapshot itself: a jet older than the test setting rows needs a
# newer SUITE_RUNNER, which keeps the slicing, baselines and verdicts while the snapshot builds and runs rows.
# SUITE_OUT defaults to $JET_SCRATCH_ROOT/lanes/Main/q/sg. Roots: Tools/agent/roots.sh.
# jetpack: the corpora use the jetpack beside the snapshot's jet. A snapshot without one gets JETPACK when set,
# else base/SHA/jetpack, where SHA is JETDIR/COMMIT, else TREE/.chain-base (a hosted engine built from the
# same commit as the jet under test). publish-base.sh gives every published base its jetpack; a cases or
# corpora run without one prints WARN first and last.
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
jetpack=""
warn=""
if [ -n "${JETPACK:-}" ]; then
  jetpack=$JETPACK; echo "jetpack: JETPACK $jetpack"
elif [ -e "$jetdir/jetpack" ]; then
  echo "jetpack: beside $jetdir/jet"
elif [ -n "$sha" ] && [ -x "$B/$sha/jetpack" ]; then
  jetpack=$B/$sha/jetpack; echo "jetpack: $jetpack"
elif [ "$suite" = cases ] || [ "$suite" = corpora ]; then
  # Fall back to the newest published jetpack (a STAND-IN) so engine cases compare like for like with the
  # base run instead of erroring; a wave that changes Jetpack must still be proved with its own jetpack.
  standin=$(ls -td "$B"/*/ 2>/dev/null | while read -r d; do [ -x "$d/jetpack" ] && { readlink -f "$d/jetpack"; break; }; done)
  if [ -n "$standin" ]; then
    jetpack=$standin
    warn="WARN: STAND-IN JETPACK for ${sha:-unknown commit}: no $B/${sha:-?}/jetpack, using $standin; Jetpack source changes in this tree are NOT exercised"
  else
    warn="WARN: NO JETPACK for ${sha:-unknown commit} (no $B/${sha:-?}/jetpack): ~156 jetpack cases rows will ERROR 'jetpack binary required'; build it with: SLOT_SMALL=0 Tools/agent/chain/build-slot.sh bash Tools/agent/land/publish-base.sh --jetpack-run ${sha:-COMMIT}"
  fi
  echo "$warn"; echo "$warn" >&2
else echo "jetpack: none (profile $suite does not use it)"; fi
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
# The corpora find their engine beside the compiler before PATH: give a snapshot without one the jetpack
# chosen above, or engine-dispatch cases reach a stale PATH jetpack and fail E1227 (wave 11, 2026-10-31).
if [ -n "$jetpack" ] && [ ! -e "$sg/jet-$tag/jetpack" ]; then ln -s "$jetpack" "$sg/jet-$tag/jetpack"; fi
runner=${SUITE_RUNNER:-$sg/jet-$tag/jet}
echo "runner: $runner"
cd "$tree" || exit 1
ID=$(awk 1 "$B/latest/IDENTITY")
rm -rf "$fs/sc-$suite-$tag"
mkdir -p "$fs/sc-$suite-$tag"
# The pool sizes itself: one entry per core while their memory claims (history peaks) fit the room under
# the build slot's cgroup caps, which it reads itself (2026-11-01: a pool sized from the host's MemAvailable took
# the machine into swap). SUITE_JOBS and SUITE_MEMORY (e.g. 8G) override the count and the budget.
pool=()
[ -n "${SUITE_JOBS:-}" ] && pool+=("--jobs=$SUITE_JOBS")
[ -n "${SUITE_MEMORY:-}" ] && pool+=("--memory=$SUITE_MEMORY")
# Suite rows run host tools (C compilers, rustc, interpreters) that only jet-env supplies until jet does.
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=$PWD JET_TEST_SCRATCH_DIR=$fs/sc-$suite-$tag TMPDIR=$fs/tmp timeout 6800 \
  "$tree/Tools/agent/jet-env" "$runner" test Compiler --profile "$suite" "--compiler=$sg/jet-$tag/jet" "${pool[@]}" "$@" \
  > "$sg/$suite-$tag.txt" 2>&1
rc=$?
echo "rc=$rc $(awk 'END{print}' "$sg/$suite-$tag.txt" | cut -c1-240)"
[ -z "$warn" ] || echo "$warn"
