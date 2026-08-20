#!/usr/bin/env bash
# proof-parallel.sh — run many targeted suites at once instead of end to end.
#
# Why: the orchestrator's proof pass was serial. One pass over fifteen suites
# cost ~25 minutes of wall clock on a 16-core machine that was idle for most of
# it, because each `cargo test` invocation waits for the previous one's build
# lock even when nothing needs building.
#
# So: build every test binary ONCE, then execute the suites concurrently. The
# build lock is held once, up front; after that the runs only compete for CPU.
#
# Usage:
#   scripts/agent/proof-parallel.sh SUITE...            # cargo test --test SUITE
#   scripts/agent/proof-parallel.sh -j 4 SUITE...       # cap concurrency
#   scripts/agent/proof-parallel.sh --crate jet-sema    # a crate's lib tests
#
# Output: one PASS/FAIL line per suite plus a log path per failure. Exit code is
# nonzero when any suite failed, so a caller can gate on it.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1
export JET_NIX_TMP_CLEANED=1

jobs=6
suites=()
crates=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    -j) jobs="$2"; shift 2 ;;
    --crate) crates+=("$2"); shift 2 ;;
    *) suites+=("$1"); shift ;;
  esac
done

if [ "${#suites[@]}" -eq 0 ] && [ "${#crates[@]}" -eq 0 ]; then
  echo "usage: proof-parallel.sh [-j N] [--crate NAME]... SUITE..." >&2
  exit 2
fi

logdir="${JET_PROOF_LOGS:-$PWD/target-proof-logs}"
mkdir -p "$logdir"

echo "== building test binaries once"
build_log="$logdir/build.log"
if ! timeout 3000 scripts/agent/jet-env cargo test --no-run --workspace >"$build_log" 2>&1; then
  echo "BUILD FAILED — see $build_log"
  node -e 'const fs=require("fs");const s=fs.readFileSync(process.argv[1],"utf8");const e=[...s.matchAll(/^error[^\n]*/gm)].map(m=>m[0]);console.log(e.slice(0,8).join("\n"))' "$build_log"
  exit 1
fi
echo "== binaries ready; running ${#suites[@]} suite(s) and ${#crates[@]} crate(s) with -j $jobs"

pids=()
labels=()
start_one() {
  label="$1"; shift
  log="$logdir/${label//[^A-Za-z0-9_.-]/_}.log"
  ( timeout 2400 scripts/agent/jet-env "$@" >"$log" 2>&1; echo "$?" >"$log.code" ) &
  pids+=("$!")
  labels+=("$label|$log")
}

wait_slot() {
  while [ "$(jobs -rp | wc -l)" -ge "$jobs" ]; do
    sleep 1
  done
}

for s in "${suites[@]+"${suites[@]}"}"; do
  wait_slot
  start_one "t_$s" cargo test --test "$s"
done
for c in "${crates[@]+"${crates[@]}"}"; do
  wait_slot
  start_one "c_$c" cargo test -p "$c"
done
wait

fail=0
echo
for entry in "${labels[@]}"; do
  label="${entry%%|*}"
  log="${entry#*|}"
  code="$(cat "$log.code" 2>/dev/null || echo 1)"
  summary="$(node -e '
const fs=require("fs");
let s="";try{s=fs.readFileSync(process.argv[1],"utf8")}catch(e){}
const res=[...s.matchAll(/test result:[^\n]*/g)].map(m=>m[0]);
const failed=[...s.matchAll(/^---- (\S+) stdout ----$/gm)].map(m=>m[1]);
const pass=res.reduce((a,r)=>a+ +((r.match(/(\d+) passed/)||[0,0])[1]),0);
const bad=res.reduce((a,r)=>a+ +((r.match(/(\d+) failed/)||[0,0])[1]),0);
console.log(`${pass} passed, ${bad} failed${failed.length?" | "+failed.slice(0,4).join(" "):""}`);
' "$log")"
  if [ "$code" = "0" ]; then
    printf 'PASS  %-34s %s\n' "$label" "$summary"
  else
    fail=1
    printf 'FAIL  %-34s %s\n      log: %s\n' "$label" "$summary" "$log"
  fi
done

exit "$fail"
