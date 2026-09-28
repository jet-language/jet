#!/usr/bin/env bash
# Jetpack bootstrap proof: assemble the explicit Jetpack Jet unit and run the
# repository's supported `jet` commands on it, mirroring Compiler/Bootstrap.
#
#   Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh                    # jet check, every area
#   Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh --tests            # + AOT and default-tier tests
#   JETPACK_WORKER=<name> ... check.sh --areas Foundation,PackageModel     # only these sections
#
# Workers sharing one checkout pass --areas (their area plus the areas it
# depends on) and JETPACK_WORKER, so another writer's unfinished files and
# scratch directory never affect their proof.
#
# Every jet command runs in its own cgroup scope with a hard memory cap and no
# swap, under a wall-clock timeout; AOT builds take one of a few shared slots.
# Assembled units, logs and receipts stay under ~/.cache/jet-luna, never /tmp.
set -euo pipefail

bootstrap="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$bootstrap/../.." && pwd)"
source_root="${JETPACK_BOOTSTRAP_SOURCE_ROOT:-$repo}"
with_tests=0
areas=""
while (( $# > 0 )); do
  case "$1" in
    --tests) with_tests=1 ;;
    --areas) areas="${2:?--areas needs a comma-separated list}"; shift ;;
    --areas=*) areas="${1#--areas=}" ;;
    *) echo "usage: check.sh [--tests] [--areas Area,Area]" >&2; exit 64 ;;
  esac
  shift
done
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

root_tag="$(printf '%s\0%s' "$(cd "$source_root" && pwd)" "${sorted_areas:-*}" | sha256sum | cut -c1-10)"
scratch="$HOME/.cache/jet-luna/jetpack-bootstrap/$worker-$root_tag"
export JETPACK_BOOTSTRAP_SOURCE_ROOT="$source_root" JETPACK_BOOTSTRAP_SCRATCH="$scratch" JETPACK_WORKER="$worker"
if [[ -n "$sorted_areas" ]]; then export JETPACK_BOOTSTRAP_AREAS="$sorted_areas"; else unset JETPACK_BOOTSTRAP_AREAS; fi
receipt="$scratch/check.receipt"
mkdir -p "$scratch" "$HOME/.cache/jet-luna/jetpack-bootstrap/slots"

check_mem="${JETPACK_CHECK_MEM:-6G}"
aot_mem="${JETPACK_AOT_MEM:-10G}"
floor_mb="${JETPACK_MIN_FREE_MB:-12000}"

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
  printf 'entry-command: Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh%s%s\n' "$([[ $with_tests == 1 ]] && echo ' --tests')" "$([[ -n $sorted_areas ]] && echo " --areas $sorted_areas")"
  printf 'worker: %s\n' "$worker"
  printf 'areas: %s\n' "${sorted_areas:-all}"
  printf 'input-source-root: %s\n' "$source_root"
  printf 'source-head: %s\n' "$(git -C "$source_root" rev-parse HEAD 2>/dev/null || echo unknown)"
  printf 'jet-binary: %s\n' "$jet"
  printf 'jet-binary-mtime: %s\n' "$(stat -c %y "$jet")"
  printf 'sources-list-sha256: %s\n' "$(sha256sum "$source_root/Jetpack/Bootstrap/sources.list" | cut -d' ' -f1)"
  printf 'tests-list-sha256: %s\n' "$(sha256sum "$source_root/Jetpack/Bootstrap/tests.list" | cut -d' ' -f1)"
} > "$receipt"

overall=0
node "$bootstrap/assemble.mjs" check
unit_record check
bounded check "$check_mem" 1200 "$scratch/check/project" "$jet" check src/jetpack.jet || overall=1

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
