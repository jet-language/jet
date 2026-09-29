#!/usr/bin/env bash
# Jetpack bootstrap proof: assemble the explicit Jetpack Jet unit and run the
# repository's supported `jet` commands on it, mirroring Compiler/Bootstrap.
#
#   Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh                    # jet check, one unit of every area
#   Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh --tests            # + AOT and default-tier tests
#   JETPACK_WORKER=<name> ... check.sh --area PackageModel [--tests]       # one area + its declared deps
#   ... check.sh --each [--tests]                                          # every area as its own unit
#   JETPACK_WORKER=<name> ... check.sh --areas Foundation,PackageModel     # an explicit section list
#
# Jetpack/Bootstrap/areas.list declares which areas each area may use. An area's
# unit holds only itself and that closure, so code cannot reach an undeclared
# area, and another writer's unfinished area cannot break an unrelated proof.
# JETPACK_WORKER gives each concurrent writer its own scratch directory.
#
# Every jet command runs in its own cgroup scope with a hard memory cap and no
# swap, under a wall-clock timeout; AOT builds take one of a few shared slots.
# Assembled units, logs and receipts stay under ~/.cache/jet-luna, never /tmp.
set -euo pipefail

bootstrap="${JETPACK_BOOTSTRAP_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
# Run from a private copy: bash reads scripts incrementally, so an edit to this
# file while a long proof runs would otherwise corrupt the running proof.
if [[ -z "${JETPACK_CHECK_SNAPSHOT:-}" ]]; then
  snapshot_dir="$HOME/.cache/jet-luna/jetpack-bootstrap/scripts"
  mkdir -p "$snapshot_dir"
  snapshot="$snapshot_dir/check-$$.sh"
  cp "$bootstrap/check.sh" "$snapshot"
  JETPACK_CHECK_SNAPSHOT="$snapshot" JETPACK_BOOTSTRAP_DIR="$bootstrap" exec bash "$snapshot" "$@"
fi
trap 'rm -f "$JETPACK_CHECK_SNAPSHOT"' EXIT
unset JETPACK_BOOTSTRAP_DIR
repo="$(cd "$bootstrap/../.." && pwd)"
source_root="${JETPACK_BOOTSTRAP_SOURCE_ROOT:-$repo}"
with_tests=0
areas=""
own_area=""
each=0
deps_from_head=0
syntax_only=0
while (( $# > 0 )); do
  case "$1" in
    --tests) with_tests=1 ;;
    --syntax) syntax_only=1 ;;
    --areas) areas="${2:?--areas needs a comma-separated list}"; shift ;;
    --areas=*) areas="${1#--areas=}" ;;
    --area) own_area="${2:?--area needs an area name}"; areas="$(node "$bootstrap/areas.mjs" closure "$own_area")"; shift ;;
    --area=*) own_area="${1#--area=}"; areas="$(node "$bootstrap/areas.mjs" closure "$own_area")" ;;
    --each) each=1 ;;
    --deps-from-head) deps_from_head=1 ;;
    *) echo "usage: check.sh [--syntax | --tests] [--area Area [--deps-from-head] | --areas Area,Area | --each]" >&2; exit 64 ;;
  esac
  shift
done
if (( each == 1 )); then
  # One unit per populated area, a few at a time; each child caps its own memory.
  parallel="${JETPACK_EACH_PARALLEL:-4}"
  base_worker="${JETPACK_WORKER:-lead}"
  summary="$HOME/.cache/jet-luna/jetpack-bootstrap/$base_worker-each.summary"
  : > "$summary"
  while IFS= read -r area; do
    [[ -n "$area" ]] || continue
    while (( $(jobs -rp | wc -l) >= parallel )); do wait -n || true; done
    (
      JETPACK_WORKER="$base_worker-each-$area" env -u JETPACK_CHECK_SNAPSHOT bash "$bootstrap/check.sh" --area "$area" $([[ $with_tests == 1 ]] && echo --tests) \
        > "$HOME/.cache/jet-luna/jetpack-bootstrap/$base_worker-each-$area.log" 2>&1
      printf '%s %d\n' "$area" "$?" >> "$summary"
    ) &
  done < <(node "$bootstrap/areas.mjs" populated)
  wait
  failed=0
  while read -r area status; do
    printf 'area %-14s %s (log ~/.cache/jet-luna/jetpack-bootstrap/%s-each-%s.log)\n' "$area" "$([[ $status == 0 ]] && echo OK || echo "FAILED exit $status")" "$base_worker" "$area"
    [[ "$status" == 0 ]] || failed=1
  done < <(sort "$summary")
  (( failed == 0 )) && echo "JETPACK EACH OK$([[ $with_tests == 1 ]] && echo ' (check, default run, AOT test)')"
  exit "$failed"
fi
worker="${JETPACK_WORKER:-lead}"
if [[ ! "$worker" =~ ^[A-Za-z0-9_-]+$ ]]; then
  echo "jetpack-bootstrap: JETPACK_WORKER must be a plain name" >&2
  exit 64
fi
sorted_areas="$(printf '%s' "$areas" | tr ',' '\n' | sed 's/^ *//;s/ *$//' | sed '/^$/d' | sort | paste -sd, -)"

# Worktrees have no local build: use the main checkout's compiler.
common="$(git -C "$repo" rev-parse --path-format=absolute --git-common-dir)"
jet="${JETPACK_JET_BIN:-$(dirname "$common")/target/debug/jet}"
if [[ ! -x "$jet" ]]; then
  echo "jetpack-bootstrap: no jet compiler at $jet (set JETPACK_JET_BIN)" >&2
  exit 69
fi

root_tag="$(printf '%s\0%s\0%s\0%s' "$(cd "$source_root" && pwd)" "${sorted_areas:-*}" "$own_area" "$deps_from_head" | sha256sum | cut -c1-10)"
scratch="$HOME/.cache/jet-luna/jetpack-bootstrap/$worker-$root_tag"
export JETPACK_BOOTSTRAP_SOURCE_ROOT="$source_root" JETPACK_BOOTSTRAP_SCRATCH="$scratch" JETPACK_WORKER="$worker"
if [[ -n "$sorted_areas" ]]; then export JETPACK_BOOTSTRAP_AREAS="$sorted_areas"; else unset JETPACK_BOOTSTRAP_AREAS; fi
if [[ -n "$own_area" ]]; then export JETPACK_BOOTSTRAP_OWN_AREA="$own_area"; else unset JETPACK_BOOTSTRAP_OWN_AREA; fi
if (( deps_from_head == 1 )); then
  [[ -n "$own_area" ]] || { echo "jetpack-bootstrap: --deps-from-head needs --area" >&2; exit 64; }
  export JETPACK_BOOTSTRAP_DEPS_FROM_HEAD=1
else
  unset JETPACK_BOOTSTRAP_DEPS_FROM_HEAD
fi
receipt="$scratch/check.receipt"
mkdir -p "$scratch" "$HOME/.cache/jet-luna/jetpack-bootstrap/slots"
# Pin one compiler for every stage: a hard link keeps this exact binary even if
# cargo replaces target/debug/jet mid-proof (no extra disk; removed at exit).
jet_pinned="$scratch/jet-pinned"
ln -f "$jet" "$jet_pinned"
jet_source="$jet"
jet="$jet_pinned"
trap 'rm -f "$JETPACK_CHECK_SNAPSHOT" "$jet_pinned"' EXIT

check_mem="${JETPACK_CHECK_MEM:-6G}"
aot_mem="${JETPACK_AOT_MEM:-10G}"
floor_mb="${JETPACK_MIN_FREE_MB:-16000}"

available_mb() { awk '/MemAvailable/ { print int($2 / 1024) }' /proc/meminfo; }

wait_for_memory() {
  local waited=0
  while (( $(available_mb) < floor_mb )); do
    if (( waited >= 600 )); then
      echo "jetpack-bootstrap: MemAvailable stayed below ${floor_mb} MiB for 10 minutes; refusing to start" >&2
      exit 75
    fi
    sleep 10
    waited=$((waited + 10))
  done
}

# bounded <label> <mem> <secs> <workdir> cmd... : one capped cgroup scope; logs
# the command output and records exit status and peak memory in the receipt.
bounded() {
  local label="$1" mem="$2" secs="$3" workdir="$4"
  shift 4
  local log="$scratch/$label.log" status peak
  wait_for_memory
  printf -v quoted '%q ' "$@"
  printf '%s-command: %s\n' "$label" "${quoted% }" >> "$receipt"
  set +e
  (
    cd "$workdir"
    systemd-run --user --scope --quiet --expand-environment=no -p MemoryMax="$mem" -p MemorySwapMax=0 \
      bash -c 'timeout --foreground "$0" "${@}"; status=$?; cg="/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)"; echo "peak_bytes=$(cat "$cg/memory.peak" 2>/dev/null || echo unknown)" >&2; exit $status' \
      "$secs" "$@"
  ) > "$log" 2>&1
  status=$?
  set -e
  peak="$(sed -n 's/^peak_bytes=//p' "$log" | tail -n1)"
  printf '%s-exit: %d\n%s-peak-bytes: %s\n%s-log: %s\n' "$label" "$status" "$label" "${peak:-unknown}" "$label" "$log" >> "$receipt"
  grep -v '^peak_bytes=' "$log" || true
  return "$status"
}

unit_record() {
  local mode="$1" project="$scratch/$1/project"
  printf '%s-unit-sha256: %s\n' "$mode" "$(sha256sum "$project/src/jetpack.jet" | cut -d' ' -f1)" >> "$receipt"
  printf '%s-source-map: %s\n' "$mode" "$scratch/$mode/jetpack.map.json" >> "$receipt"
}

{
  selection=""
  if [[ -n "$own_area" ]]; then
    selection=" --area $own_area"
    if (( deps_from_head == 1 )); then selection="$selection --deps-from-head"; fi
  elif [[ -n "$sorted_areas" ]]; then
    selection=" --areas $sorted_areas"
  fi
  printf 'entry-command: JETPACK_WORKER=%s Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh%s%s\n' "$worker" "$([[ $with_tests == 1 ]] && echo ' --tests')" "$selection"
  printf 'worker: %s\n' "$worker"
  printf 'areas: %s\n' "${sorted_areas:-all}"
  printf 'proved-area: %s\n' "${own_area:-all selected}"
  printf 'input-source-root: %s\n' "$source_root"
  printf 'source-head: %s\n' "$(git -C "$source_root" rev-parse HEAD 2>/dev/null || echo unknown)"
  printf 'jet-binary: %s (pinned as %s)\n' "$jet_source" "$jet"
  printf 'jet-binary-mtime: %s\n' "$(stat -c %y "$jet")"
  printf 'jet-binary-sha256: %s\n' "$(sha256sum "$jet" | cut -d' ' -f1)"
  printf 'sources-list-sha256: %s\n' "$(sha256sum "$source_root/Jetpack/Bootstrap/sources.list" | cut -d' ' -f1)"
  printf 'tests-list-sha256: %s\n' "$(sha256sum "$source_root/Jetpack/Bootstrap/tests.list" | cut -d' ' -f1)"
} > "$receipt"

overall=0
node "$bootstrap/assemble.mjs" check
unit_record check
if (( ${syntax_only:-0} == 1 )); then
  # Grammar only (lexer + parser, well under a second): the fast build-out loop.
  probe="$(dirname "$jet_source")/jet-bootstrap-syntax-probe"
  # TRANSITIONAL (D-TYPE-SUFFIX1, D-CAP-RECEIVER1): the probe predates both cutovers.
  node "$bootstrap/precutover.mjs" "$scratch/check/project/src/jetpack.jet" "$scratch/check/jetpack.precutover.jet"
  systemd-run --user --scope --quiet --expand-environment=no -p MemoryMax=3G -p MemorySwapMax=0 \
    "$probe" "$scratch/check/jetpack.precutover.jet" 2>&1 | node "$bootstrap/locate.mjs" syntax "$scratch/check/jetpack.map.json" | tee "$scratch/syntax.log"
  grep -q '^total errors: 0$' "$scratch/syntax.log" || overall=1
  printf 'syntax-log: %s\nexit: %d\n' "$scratch/syntax.log" "$overall" >> "$receipt"
  (( overall == 0 )) && echo "JETPACK SYNTAX OK"
  exit "$overall"
fi
# Full type checks take a machine-wide slot (JETPACK_CHECK_SLOTS, default 2):
# many parallel writers must not run many multi-GB checks at once.
check_slots="${JETPACK_CHECK_SLOTS:-2}"
exec 7>"$HOME/.cache/jet-luna/jetpack-bootstrap/slots/check-wait"
got_slot=""
while [[ -z "$got_slot" ]]; do
  for slot in $(seq 1 "$check_slots"); do
    exec 8>"$HOME/.cache/jet-luna/jetpack-bootstrap/slots/check-$slot"
    if flock -n 8; then got_slot="$slot"; break; fi
    exec 8>&-
  done
  [[ -n "$got_slot" ]] || { flock 7; sleep 5; flock -u 7; }
done
bounded check "$check_mem" 1200 "$scratch/check/project" "$jet" check src/jetpack.jet || overall=1
flock -u 8; exec 8>&-
# Per-file error summary mapped back to source paths (advisory type check).
node "$bootstrap/locate.mjs" check "$scratch/check/jetpack.map.json" "$scratch/check.log" > "$scratch/check-errors.txt" || true
if (( overall != 0 )); then echo "--- errors by source file ($scratch/check-errors.txt)"; grep -A200 '^errors per file:' "$scratch/check-errors.txt" || true; fi

if (( with_tests == 1 && overall == 0 )); then
  node "$bootstrap/assemble.mjs" run
  unit_record run
  if bounded run "$check_mem" 1800 "$scratch/run/project" "$jet" run src/jetpack.jet; then
    expected="$(node -e 'console.log(require(process.argv[1]).tests.length)' "$scratch/run/jetpack.map.json")"
    if ! grep -qx "jetpack-bootstrap: $expected claims passed" "$scratch/run.log"; then
      echo "jetpack-bootstrap: default tier did not report all $expected claims" >&2
      overall=1
    fi
  else
    overall=1
  fi
  rm -rf "$scratch/run/project/.jet/build"

  node "$bootstrap/assemble.mjs" aot
  unit_record aot
  # One AOT build at a time machine-wide for this stream (~1 GiB+ per build).
  exec 9>"$HOME/.cache/jet-luna/jetpack-bootstrap/slots/aot"
  flock 9
  bounded aot "$aot_mem" 1800 "$scratch/aot/project" "$jet" test src/jetpack.jet || overall=1
  flock -u 9
  rm -rf "$scratch/aot/project/.jet/build"
fi

printf 'exit: %d\n' "$overall" >> "$receipt"
echo "jetpack-bootstrap receipt: $receipt"
if (( overall == 0 )); then
  echo "JETPACK CHECK OK$([[ $with_tests == 1 ]] && echo ' (check, default run, AOT test)')"
fi
exit "$overall"
