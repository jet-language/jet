#!/usr/bin/env bash
# census-snap.sh <file>...: collect-all census of a SNAPSHOT: HEAD of the dev worktree plus the named
# worktree files (the orchestrator's own uncommitted edits), so other lanes' uncommitted work in the
# shared tree never enters it. Output: $JET_SCRATCH_ROOT/discovery/r14/census/{check.*,build.*,selfcheck.*,runtime/}.
# Passes, all in package form in the snapshot: `jet check Compiler/JetCli` on the seed (sema of the
# CLI and every package it depends on), then `jet build Compiler` with keep-going, so optimize,
# lowering, object and link rows are caught before a commit lands (a check-only gate let two build
# breaks through), then the runtime pack and the self-check with the compiler just built.
# CENSUS_BUILD=0 skips everything after the check. CENSUS_JET_DIR names another compiler install
# (default: the seed, $JET_SEED). Exit: 0 clean; 1 rows/failure; 2 setup failure. This is the preflight
# AGENTS.md names; precommit.sh runs it for every compiler-input commit. Roots: Tools/agent/roots.sh.
# A clean census of every pass (no CENSUS_BUILD=0 or CENSUS_SELFCHECK=0) writes the snapshot's preflight
# receipt (preflight-receipt.mjs), which chain.sh full, release-compiler.mjs and publish-release.mjs require.
# Never edit this file in place while it runs (bash reads scripts incrementally): write a new file and mv.
set -uo pipefail
# Its own lock: precommit censuses must never queue behind lanes' long checks (which hold census.lock).
M=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
. "$M/../roots.sh"
mkdir -p "$JET_RUN_ROOT"
exec 9>"$JET_RUN_ROOT/precommit-census.lock"; flock 9
W=$JET_DEV_WORKTREE
S=$JET_SCRATCH_ROOT/discovery/r14/snap
OUT=$JET_SCRATCH_ROOT/discovery/r14/census
C=${CENSUS_JET_DIR:-$(readlink -f "$JET_SEED")}
ID=${CENSUS_JET_IDENTITY:-$(cat "$C/IDENTITY")}
rm -rf "$S" "$OUT"; mkdir -p "$S" "$OUT"
# CENSUS_COMMIT=<sha> censuses that commit instead of HEAD (a batch check of an exact commit).
HEAD_SHA=$(git -C "$W" rev-parse "${CENSUS_COMMIT:-HEAD}^{commit}") || { echo "EXPORT FAILED"; exit 2; }
( cd "$W" && git archive "$HEAD_SHA" | tar -x -C "$S" ) || { echo "EXPORT FAILED"; exit 2; }
# A named file the worktree no longer has is a deletion: remove it from the snapshot too.
for f in "$@"; do if [ -e "$W/$f" ]; then mkdir -p "$S/$(dirname "$f")" && cp "$W/$f" "$S/$f"; else rm -f "$S/$f"; fi; done
cd "$S" || exit 2
# The receipt's key: the snapshot's compiler source identity, before any pass writes into it.
SID=$(node "$M/preflight-receipt.mjs" identity "$S") || { echo "SOURCE IDENTITY FAILED"; exit 2; }
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=${CENSUS_TOOLCHAIN_ROOT:-$S} JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/check-keep-going.tsv" \
  prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 "$C/jet" check --threads 12 Compiler/JetCli > "$OUT/check.out" 2> "$OUT/check.err"
crc=$?
echo "check rc=$crc"
# The keep-going report is the census: a dependency package's errors reach the stderr only as E0626.
node "$M/census-errors.mjs" "$OUT/check-keep-going.tsv" "$S" 2>&1 | awk 'NR<=21' | cut -c1-200
[ "$crc" = 0 ] || exit 1
[ "${CENSUS_BUILD:-1}" = 0 ] && exit 0
rm -rf Compiler/JetCli/.jet/build
JET_COMPILER_IDENTITY=$ID JET_TOOLCHAIN_ROOT=${CENSUS_BUILD_ROOT:-$S} \
  JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/build-keep-going.tsv" \
  timeout 1800 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 "$C/jet" build --threads 12 Compiler > "$OUT/build.out" 2> "$OUT/build.err"
brc=$?
rows=$(awk 'NR>1' "$OUT/build-keep-going.tsv" 2>/dev/null | wc -l)
echo "build rc=$brc rows=$rows"
awk -F'\t' 'NR>1{print $1"\t"$3"\t"$4}' "$OUT/build-keep-going.tsv" 2>/dev/null | sort | uniq -c | sort -rn | cut -c1-200 | head -10
[ "$brc" = 0 ] && [ "$rows" = 0 ] || { head -c 600 "$OUT/build.err"; exit 1; }
# The compiler `jet build Compiler` just built (JetCli's executable target `jet`).
SELF=$S/Compiler/JetCli/.jet/build/jet
# Runtime pass, before the self-check and on every compiler census: the freshly built compiler lowers the
# runtime pack exactly as chain s1's pack step does. Compile-time evaluation runs runtime routes on the
# pack's own IR (runtime.ir, #5073 A3), which the compiler reads from `jet-runtime/` beside its
# executable, so the self-check below (like chain s2) needs this pack; any failure fails the commit.
bash "$M/runtime-pack-check.sh" "$SELF" "$S" "$OUT/runtime" "$ID" || exit 1
ln -sfn "$OUT/runtime/out" "$(dirname "$(readlink -f "$SELF")")/jet-runtime"
# Self-check pass (CENSUS_SELFCHECK=0 skips): the compiler just built checks the compiler packages with
# keep-going, as chain s2 does; a sema/comptime row here would stop s2 (e.g. an unlowered inline module).
if [ "${CENSUS_SELFCHECK:-1}" = 1 ]; then
  JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$S" JET_KEEP_GOING=1 JET_KEEP_GOING_REPORT="$OUT/selfcheck-keep-going.tsv" \
    timeout 1200 prlimit --stack=1073741824:1073741824 -- taskset -c 16-27 "$SELF" check --threads 12 Compiler/JetCli > "$OUT/selfcheck.out" 2> "$OUT/selfcheck.err"
  src=$?
  srows=$(awk 'NR>1' "$OUT/selfcheck-keep-going.tsv" 2>/dev/null | wc -l)
  echo "selfcheck rc=$src rows=$srows"
  node "$M/census-errors.mjs" "$OUT/selfcheck-keep-going.tsv" "$S" 2>&1 | awk 'NR<=11' | cut -c1-200
  [ "$src" = 0 ] && [ "$srows" = 0 ] || exit 1
  node "$M/preflight-receipt.mjs" write "$SID" "$(cat "$C/IDENTITY")" "$HEAD_SHA" "$@" || exit 2
fi
exit 0
