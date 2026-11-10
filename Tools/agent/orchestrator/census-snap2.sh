#!/usr/bin/env bash
# census-snap.sh <file>...: collect-all census of a SNAPSHOT: HEAD of the dev worktree plus the named
# worktree files (the orchestrator's own uncommitted edits), so other lanes' uncommitted work in the
# shared tree never enters it. Output: $JET_SCRATCH_ROOT/discovery/r14p/census/{check.*,build.*,selfcheck.*,runtime/}.
# Passes, all in package form in the snapshot: `jet check Compiler/JetCli` on the seed (sema of the
# CLI and every package it depends on), then `jet build Compiler` with keep-going, so optimize,
# lowering, object and link rows are caught before a commit lands (a check-only gate let two build
# breaks through), then the runtime pack and the self-check with the compiler just built.
# CENSUS_BUILD=0 skips everything after the check. CENSUS_JET_DIR names another compiler install
# (default: the seed). Exit: 0 clean; 1 rows/failure; 2 setup failure.
# Never edit this file in place while it runs (bash reads scripts incrementally): write a new file and mv.
# The original of gauntlet tool entry census-snap2; CensusSnap2.jet beside it is its Jet twin. The dev
# worktree is $JET_DEV_WORKTREE, the seed $JET_SEED, the lock is in $JET_RUN_ROOT, and jetenv.sh,
# census-errors.mjs, guards-gate.mjs, guards-known.txt, runtime-pack-check.sh and jpwrap.sh are Main's,
# in $JET_STATE_ROOT/scratch/Main (Tools/agent/roots.sh).
set -uo pipefail
. "$(dirname "$0")/../roots.sh"
G=$JET_STATE_ROOT/scratch/Main
# Its own lock AND its own directories: precommit censuses never queue behind lanes' long checks, and
# never share r14 with Tools/agent/land/census-snap.sh (vbatch's launch census), whose different lock
# let a precommit delete the runtime pack directory under a running batch census (vb6).
mkdir -p "$JET_RUN_ROOT"; exec 9>"$JET_RUN_ROOT/precommit-census.lock"; flock 9
W=$JET_DEV_WORKTREE
# CENSUS_TREE names a prepared tree (integrate.sh: origin/dev plus submitted patches) used as is, with
# CENSUS_OUT for its reports; the named files then only select the Jetpack pass.
S=${CENSUS_TREE:-$JET_SCRATCH_ROOT/discovery/r14p/snap}
OUT=${CENSUS_OUT:-$JET_SCRATCH_ROOT/discovery/r14p/census}
C=${CENSUS_JET_DIR:-$(readlink -f "$JET_SEED")}
ID=${CENSUS_JET_IDENTITY:-$(cat "$C/IDENTITY")}
if [ -n "${CENSUS_TREE:-}" ]; then rm -rf "$OUT"; mkdir -p "$OUT"; else
rm -rf "$S" "$OUT"; mkdir -p "$S" "$OUT"
( cd "$W" && git archive HEAD | tar -x -C "$S" ) || { echo "EXPORT FAILED"; exit 2; }
# A named file the worktree no longer has is a deletion: remove it from the snapshot too.
for f in "$@"; do if [ -e "$W/$f" ]; then mkdir -p "$S/$(dirname "$f")" && cp "$W/$f" "$S/$f"; else rm -f "$S/$f"; fi; done
fi
cd "$S" || exit 2
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=${CENSUS_TOOLCHAIN_ROOT:-$S} JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/check-keep-going.tsv" \
  prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$C/jet" check --threads 12 Compiler/JetCli > "$OUT/check.out" 2> "$OUT/check.err"
crc=$?
echo "check rc=$crc"
# The keep-going report is the census: a dependency package's errors reach the stderr only as E0626.
node "$G/census-errors.mjs" "$OUT/check-keep-going.tsv" "$S" 2>&1 | awk 'NR<=21' | cut -c1-200
[ "$crc" = 0 ] || exit 1
[ "${CENSUS_BUILD:-1}" = 0 ] && exit 0
rm -rf Compiler/JetCli/.jet/build
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=${CENSUS_BUILD_ROOT:-$S} \
  JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/build-keep-going.tsv" \
  timeout 1800 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$C/jet" build --threads 12 Compiler > "$OUT/build.out" 2> "$OUT/build.err"
brc=$?
rows=$(awk 'NR>1' "$OUT/build-keep-going.tsv" 2>/dev/null | wc -l)
echo "build rc=$brc rows=$rows"
awk -F'\t' 'NR>1{print $1"\t"$3"\t"$4}' "$OUT/build-keep-going.tsv" 2>/dev/null | sort | uniq -c | sort -rn | cut -c1-200 | head -10
[ "$brc" = 0 ] && [ "$rows" = 0 ] || { head -c 600 "$OUT/build.err"; exit 1; }
# Guards pass (CENSUS_GUARDS=0 skips): tests/guards on the seed, failing only on a guard row or closed-list
# problem not in guards-known.txt. Without it, 18 hand-written closed lists landed unseen (c65 run 6).
if [ "${CENSUS_GUARDS:-1}" = 1 ]; then
  # 2400 s (was 900): post42h's guards hit 900 s at machine load 62 with no failing guard.
  JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=$S timeout "${CENSUS_GUARDS_TIMEOUT:-2400}" taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$C/jet" test tests/guards > "$OUT/guards.out" 2>&1
  node "$G/guards-gate.mjs" "$OUT/guards.out" "$G/guards-known.txt"; grc=$?
  echo "guards rc=$grc"
  [ "$grc" = 0 ] || exit 1
fi
# The compiler `jet build Compiler` just built (JetCli's executable target `jet`).
SELF=$S/Compiler/JetCli/.jet/build/jet
# Runtime pass, before the self-check and on every compiler census: the freshly built compiler lowers the
# runtime pack exactly as chain s1's pack step does. Compile-time evaluation runs runtime routes on the
# pack's own IR (runtime.ir, #5073 A3), which the compiler reads from `jet-runtime/` beside its
# executable, so the self-check below (like chain s2) needs this pack; any failure fails the commit.
# A tree whose Compiler/build.jet builds the pack (Compiler/Toolchain/RuntimePack.jet) already wrote it beside
# jet during the build pass above; older trees build it here through the packager.
if [ -f "$S/Compiler/Toolchain/RuntimePack.jet" ]; then
  RP="$(dirname "$(readlink -f "$SELF")")/jet-runtime"
  [ -e "$RP" ] || [ ! -d "$S/Compiler/JetCli/.jet/build/jet-runtime" ] || ln -sfn "$S/Compiler/JetCli/.jet/build/jet-runtime" "$RP"
  if [ -f "$RP/native.key" ]; then echo "runtime rc=0 (jet build Compiler)"; else echo "runtime rc=1 (no jet-runtime/native.key beside $SELF)"; exit 1; fi
else
  bash "$G/runtime-pack-check.sh" "$SELF" "$S" "$OUT/runtime" "$ID" || exit 1
  ln -sfn "$OUT/runtime/out" "$(dirname "$(readlink -f "$SELF")")/jet-runtime"
fi
# Self-check pass (CENSUS_SELFCHECK=0 skips): the compiler just built checks the compiler packages with
# keep-going, as chain s2 does; a sema/comptime row here would stop s2 (e.g. an unlowered inline module).
if [ "${CENSUS_SELFCHECK:-1}" = 1 ]; then
  JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$S" JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/selfcheck-keep-going.tsv" \
    timeout 1200 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$SELF" check --threads 12 Compiler/JetCli > "$OUT/selfcheck.out" 2> "$OUT/selfcheck.err"
  src=$?
  srows=$(awk 'NR>1' "$OUT/selfcheck-keep-going.tsv" 2>/dev/null | wc -l)
  echo "selfcheck rc=$src rows=$srows"
  node "$G/census-errors.mjs" "$OUT/selfcheck-keep-going.tsv" "$S" 2>&1 | awk 'NR<=11' | cut -c1-200
  [ "$src" = 0 ] && [ "$srows" = 0 ] || exit 1
  # Core pass (Land.jet census selfcheck-core): every Core body, called or not, checked by the compiler just
  # built. Without it a Core spelling no compiler accepts landed (cd58a15b81 fixed it) and stopped a release preflight.
  JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$S" JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/selfcheck-core-keep-going.tsv" \
    timeout 1200 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 bash "$G/jetenv.sh" "$S" "$SELF" check --threads 12 Core > "$OUT/selfcheck-core.out" 2> "$OUT/selfcheck-core.err"
  crc=$?
  crows=$(awk 'NR>1' "$OUT/selfcheck-core-keep-going.tsv" 2>/dev/null | wc -l)
  echo "selfcheck-core rc=$crc rows=$crows"
  node "$G/census-errors.mjs" "$OUT/selfcheck-core-keep-going.tsv" "$S" 2>&1 | awk 'NR<=11' | cut -c1-200
  [ "$crc" = 0 ] && [ "$crows" = 0 ] || exit 1
fi
# Hosted Jetpack pass, when a named file is under Jetpack/: the compiler just built assembles and builds the
# hosted jetpack exactly as vbatch.sh does. The compiler census alone never compiles Jetpack, which let
# 5ef77a52ad land three E0209 rows that stopped vb6 before any suite ran.
jp=0; for f in "$@"; do case "$f" in Jetpack/*) jp=1 ;; esac; done
if [ "$jp" = 1 ] && [ "${CENSUS_JETPACK:-1}" = 1 ]; then
  JH=$(mktemp -d -p "$OUT")
  ( cd "$S" && env HOME="$JH" JET_HOME="$JH/.jet" JET="$SELF" JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$S" \
      JETPACK_WORKER=precommit JET_STATE_ROOT="$OUT/jetpack-state" timeout 1200 bash "$G/jpwrap.sh" check --hosted ) > "$OUT/jetpack.log" 2>&1
  rm -rf "$JH"
  if grep -q 'JETPACK HOSTED OK' "$OUT/jetpack.log"; then echo "jetpack rc=0"; else
    echo "jetpack rc=1 (see $OUT/jetpack.log)"; grep -E '^(Error|  -->)' "$OUT/jetpack.log" | head -20 | cut -c1-200; exit 1
  fi
fi
exit 0
