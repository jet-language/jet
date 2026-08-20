#!/usr/bin/env bash
# proof-parallel.sh — run many targeted suites at once instead of end to end.
#
# Why: the orchestrator's proof pass was serial. One pass over fifteen suites
# cost ~25 minutes of wall clock on a 16-core machine that was idle for most of
# it, because each `cargo test` invocation waits for the previous one's build
# lock even when nothing needs building. So: build every test binary ONCE, then
# execute the suites concurrently.
#
# Memory and disk safety, learned the hard way (a session ended in an OOM kill):
#   * `/tmp` is RAM-backed tmpfs here. Test batteries write multi-gigabyte
#     scratch, and eight concurrent suites put ~9G of it straight into RAM and
#     zram. TMPDIR is therefore forced to a disk path.
#   * `CARGO_INCREMENTAL=0`: incremental artifacts reached 40G in one target dir
#     and buy nothing for test binaries.
#   * The build cache is capped. One session left 517G in a single `target/`
#     (438G of it stale `deps` generations, which nothing prunes).
#   * Concurrency defaults to 4, not one-per-core: each suite can fork rustc for
#     AOT goldens, so -j N means far more than N processes.
#
# Usage:
#   scripts/agent/proof-parallel.sh SUITE...            # cargo test --test SUITE
#   scripts/agent/proof-parallel.sh -j 6 SUITE...       # raise concurrency
#   scripts/agent/proof-parallel.sh --crate jet-sema    # a crate's lib tests
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1

jobs=4
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

# ── scratch on disk, never in RAM ────────────────────────────────────────────
scratch="${JET_TEST_SCRATCH:-$HOME/.cache/jet-test-scratch}"
mkdir -p "$scratch"
case "$(df -P "$scratch" | awk 'NR==2 {print $1}')" in
  tmpfs|none)
    echo "refusing to run: scratch dir $scratch is RAM-backed; set JET_TEST_SCRATCH to a disk path" >&2
    exit 1 ;;
esac
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch"
export CARGO_INCREMENTAL=0
export JET_NIX_TMP_CLEANED=1

# Cargo defaults to one rustc per hardware thread. This machine has 32 and the
# Jet crates embed the whole Prelude, so an uncapped cold build peaked at 52G of
# 61G and drove the box into swap. Roughly 2G per rustc is the observed cost.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-8}"

# Wait until the machine can afford another heavy process.
min_free_gb="${JET_MIN_FREE_GB:-10}"
wait_for_memory() {
  while :; do
    avail=$(awk '/MemAvailable/ {print int($2/1048576)}' /proc/meminfo)
    [ "${avail:-0}" -ge "$min_free_gb" ] && return 0
    echo "   … ${avail}G available, waiting for ${min_free_gb}G"
    sleep 10
  done
}

# ── refuse to grow an already-huge cache ─────────────────────────────────────
cap_gb="${JET_TARGET_CAP_GB:-120}"
if [ -d target ]; then
  used_gb=$(du -sBG target 2>/dev/null | awk '{gsub("G","",$1); print $1+0}')
  if [ "${used_gb:-0}" -gt "$cap_gb" ]; then
    echo "refusing to run: target/ is ${used_gb}G, over the ${cap_gb}G cap." >&2
    echo "  rm -rf target/debug/incremental        # usually the biggest slice" >&2
    echo "  cargo clean                            # if that is not enough" >&2
    exit 1
  fi
fi

logdir="${JET_PROOF_LOGS:-$scratch/proof-logs}"
mkdir -p "$logdir"

echo "== scratch $scratch · target cap ${cap_gb}G · suites -j $jobs · rustc -j $CARGO_BUILD_JOBS · min free ${min_free_gb}G"
echo "== building test binaries once"
wait_for_memory
build_log="$logdir/build.log"
if ! timeout 3000 scripts/agent/jet-env cargo test --no-run --workspace >"$build_log" 2>&1; then
  echo "BUILD FAILED — see $build_log"
  node -e 'const fs=require("fs");const s=fs.readFileSync(process.argv[1],"utf8");const e=[...s.matchAll(/^error[^\n]*/gm)].map(m=>m[0]);console.log(e.slice(0,8).join("\n"))' "$build_log"
  exit 1
fi
echo "== binaries ready; running ${#suites[@]} suite(s) and ${#crates[@]} crate(s)"

labels=()
start_one() {
  label="$1"; shift
  log="$logdir/${label//[^A-Za-z0-9_.-]/_}.log"
  ( timeout 2400 scripts/agent/jet-env "$@" >"$log" 2>&1; echo "$?" >"$log.code" ) &
  labels+=("$label|$log")
}

wait_slot() {
  while [ "$(jobs -rp | wc -l)" -ge "$jobs" ]; do
    sleep 1
  done
  wait_for_memory
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

# ── give the scratch back ────────────────────────────────────────────────────
find "$scratch" -mindepth 1 -maxdepth 1 -not -name 'proof-logs' -exec rm -rf {} + 2>/dev/null
rm -rf target/tmp build/.work* 2>/dev/null
printf '\ntarget/ now %s · scratch cleared\n' "$(du -sh target 2>/dev/null | cut -f1)"

exit "$fail"
