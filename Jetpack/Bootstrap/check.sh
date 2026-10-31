#!/usr/bin/env bash
# Jetpack bootstrap proof: assemble the explicit Jetpack Jet unit and run the
# repository's supported `jet` commands on it, mirroring Compiler/Bootstrap.
# JET names the admitted native jet compiler; every stage uses it.
#
#   JET=<native jet> Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh   # jet check, one unit of every area
#   JET=<native jet> Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh --tests  # + AOT and default-tier tests
#   JETPACK_WORKER=<name> ... check.sh --area PackageModel [--tests]       # one area + its declared deps
#   ... check.sh --each [--tests]                                          # every area as its own unit
#   JETPACK_WORKER=<name> ... check.sh --areas Foundation,PackageModel     # an explicit section list
#   ... check.sh --hosted                                                  # native `jetpack` engine binary
#
# Jetpack/Bootstrap/areas.list declares which areas each area may use. An area's
# unit holds only itself and that closure, so code cannot reach an undeclared
# area, and another writer's unfinished area cannot break an unrelated proof.
# JETPACK_WORKER gives each concurrent writer its own scratch directory.
#
# `--hosted` assembles every implementation file plus Jetpack/run.jet
# (`node Jetpack/Bootstrap/assemble.mjs hosted`) and builds the standalone
# engine with `jet build`; the receipt records the binary's path and sha256.
#
# Every jet command runs in its own cgroup scope with a hard memory cap and no
# swap, under a wall-clock timeout; AOT builds take one of a few shared slots.
# Assembled units, logs and receipts stay under ~/.cache/jet-dev, never /tmp.
set -euo pipefail

bootstrap="${JETPACK_BOOTSTRAP_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
# Run from a private copy: bash reads scripts incrementally, so an edit to this
# file while a long proof runs would otherwise corrupt the running proof.
if [[ -z "${JETPACK_CHECK_SNAPSHOT:-}" ]]; then
  snapshot_dir="$HOME/.cache/jet-dev/jetpack-bootstrap/scripts"
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
hosted=0
while (( $# > 0 )); do
  case "$1" in
    --tests) with_tests=1 ;;
    --syntax) syntax_only=1 ;;
    --hosted) hosted=1 ;;
    --areas) areas="${2:?--areas needs a comma-separated list}"; shift ;;
    --areas=*) areas="${1#--areas=}" ;;
    --area) own_area="${2:?--area needs an area name}"; areas="$(node "$bootstrap/areas.mjs" closure "$own_area")"; shift ;;
    --area=*) own_area="${1#--area=}"; areas="$(node "$bootstrap/areas.mjs" closure "$own_area")" ;;
    --each) each=1 ;;
    --deps-from-head) deps_from_head=1 ;;
    *) echo "usage: check.sh [--syntax | --tests | --hosted] [--area Area [--deps-from-head] | --areas Area,Area | --each]" >&2; exit 64 ;;
  esac
  shift
done
if (( hosted == 1 )) && [[ -n "$areas" || $each == 1 || $with_tests == 1 || $syntax_only == 1 ]]; then
  echo "jetpack-bootstrap: --hosted builds the whole engine; it takes no area selection, --each, --tests or --syntax" >&2
  exit 64
fi
if (( each == 1 )); then
  # One unit per populated area, a few at a time; each child caps its own memory.
  parallel="${JETPACK_EACH_PARALLEL:-4}"
  base_worker="${JETPACK_WORKER:-lead}"
  summary="$HOME/.cache/jet-dev/jetpack-bootstrap/$base_worker-each.summary"
  mkdir -p "$(dirname "$summary")"
  : > "$summary"
  while IFS= read -r area; do
    [[ -n "$area" ]] || continue
    while (( $(jobs -rp | wc -l) >= parallel )); do wait -n || true; done
    (
      status=0
      JETPACK_WORKER="$base_worker-each-$area" env -u JETPACK_CHECK_SNAPSHOT bash "$bootstrap/check.sh" --area "$area" $([[ $with_tests == 1 ]] && echo --tests) \
        > "$HOME/.cache/jet-dev/jetpack-bootstrap/$base_worker-each-$area.log" 2>&1 || status=$?
      printf '%s %d\n' "$area" "$status" >> "$summary"
    ) &
  done < <(node "$bootstrap/areas.mjs" populated)
  wait
  failed=0
  while read -r area status; do
    printf 'area %-14s %s (log ~/.cache/jet-dev/jetpack-bootstrap/%s-each-%s.log)\n' "$area" "$([[ $status == 0 ]] && echo OK || echo "FAILED exit $status")" "$base_worker" "$area"
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

# JET names the admitted native compiler. It is an immutable candidate path
# whose runtime pack sits beside it, so it runs in place (a private copy would
# lose the runtime pack); the receipt records its sha256.
jet="${JET:-}"
if [[ -z "$jet" || ! -x "$jet" ]]; then
  echo "jetpack-bootstrap: JET must name the admitted native jet compiler" >&2
  exit 69
fi
# The compiler's identity and toolchain root are environment facts of the
# admitted compiler: the IDENTITY file beside $JET names it (the compiler refuses
# a host without JET_COMPILER_IDENTITY), and the Core library tree is the
# repository that holds this script. Explicit settings win.
if [[ -z "${JET_COMPILER_IDENTITY:-}" ]]; then
  identity_file="$(dirname "$jet")/IDENTITY"
  identity=""
  if [[ -r "$identity_file" ]]; then IFS= read -r identity < "$identity_file" || true; fi
  if [[ -z "$identity" ]]; then
    echo "jetpack-bootstrap: JET_COMPILER_IDENTITY is unset and $identity_file is missing or empty" >&2
    exit 69
  fi
  export JET_COMPILER_IDENTITY="$identity"
fi
if [[ -z "${JET_TOOLCHAIN_ROOT:-}" ]]; then
  [[ -d "$repo/Core" ]] || { echo "jetpack-bootstrap: JET_TOOLCHAIN_ROOT is unset and $repo has no Core/ tree" >&2; exit 69; }
  export JET_TOOLCHAIN_ROOT="$repo"
fi

root_tag="$(printf '%s\0%s\0%s\0%s' "$(cd "$source_root" && pwd)" "${sorted_areas:-*}" "$own_area" "$deps_from_head" | sha256sum | cut -c1-10)"
scratch="$HOME/.cache/jet-dev/jetpack-bootstrap/$worker-$root_tag"
export JETPACK_BOOTSTRAP_SOURCE_ROOT="$source_root" JETPACK_BOOTSTRAP_SCRATCH="$scratch" JETPACK_WORKER="$worker"
if [[ -n "$sorted_areas" ]]; then export JETPACK_BOOTSTRAP_AREAS="$sorted_areas"; else unset JETPACK_BOOTSTRAP_AREAS; fi
if [[ -n "$own_area" ]]; then export JETPACK_BOOTSTRAP_OWN_AREA="$own_area"; else unset JETPACK_BOOTSTRAP_OWN_AREA; fi
if (( deps_from_head == 1 )); then
  [[ -n "$own_area" ]] || { echo "jetpack-bootstrap: --deps-from-head needs --area" >&2; exit 64; }
  export JETPACK_BOOTSTRAP_DEPS_FROM_HEAD=1
else
  unset JETPACK_BOOTSTRAP_DEPS_FROM_HEAD
fi
receipt="$scratch/$( (( hosted == 1 )) && echo hosted || echo check ).receipt"
mkdir -p "$scratch" "$HOME/.cache/jet-dev/jetpack-bootstrap/slots"
trap 'rm -f "$JETPACK_CHECK_SNAPSHOT"' EXIT

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
    # RuntimeMaxSec makes systemd end the scope even if this shell dies (a killed
    # caller must never orphan an unbounded compiler); output is written from
    # inside the scope for the same reason.
    systemd-run --user --scope --quiet --expand-environment=no -p MemoryMax="$mem" -p MemorySwapMax=0 -p RuntimeMaxSec="$(( secs + 30 ))" \
      bash -c 'log="$0"; secs="$1"; shift 2; timeout --foreground "$secs" "${@}" > "$log" 2>&1; status=$?; cg="/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup)"; echo "peak_bytes=$(cat "$cg/memory.peak" 2>/dev/null || echo unknown)" >> "$log"; exit $status' \
      "$log" "$secs" -- "$@"
  )
  status=$?
  set -e
  peak="$(sed -n 's/^peak_bytes=//p' "$log" | tail -n1)"
  printf '%s-exit: %d\n%s-peak-bytes: %s\n%s-log: %s\n' "$label" "$status" "$label" "${peak:-unknown}" "$label" "$log" >> "$receipt"
  grep -v '^peak_bytes=' "$log" || true
  # The kernel ends the whole scope on a cgroup OOM, so no peak line is written.
  if (( status == 143 || status == 137 )) && [[ -z "$peak" ]]; then
    printf '%s-killed: memory cap MemoryMax=%s reached (cgroup OOM; the machine is unaffected)\n' "$label" "$mem" >> "$receipt"
    echo "jetpack-bootstrap: $label was killed at its MemoryMax=$mem cap (cgroup OOM, contained); the unit is too large to type-check under this cap (#3661)" >&2
  fi
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
  printf 'entry-command: JETPACK_WORKER=%s JET=%s Tools/agent/jet-env bash Jetpack/Bootstrap/check.sh%s%s%s\n' "$worker" "$jet" "$([[ $with_tests == 1 ]] && echo ' --tests')" "$([[ $hosted == 1 ]] && echo ' --hosted')" "$selection"
  printf 'worker: %s\n' "$worker"
  printf 'areas: %s\n' "${sorted_areas:-all}"
  printf 'proved-area: %s\n' "${own_area:-all selected}"
  printf 'input-source-root: %s\n' "$source_root"
  printf 'source-head: %s\n' "$(git -C "$source_root" rev-parse HEAD 2>/dev/null || echo unknown)"
  printf 'jet-binary: %s\n' "$jet"
  printf 'jet-binary-mtime: %s\n' "$(stat -c %y "$jet")"
  printf 'jet-binary-sha256: %s\n' "$(sha256sum "$jet" | cut -d' ' -f1)"
  printf 'sources-list-sha256: %s\n' "$(sha256sum "$source_root/Jetpack/Bootstrap/sources.list" | cut -d' ' -f1)"
  printf 'tests-list-sha256: %s\n' "$(sha256sum "$source_root/Jetpack/Bootstrap/tests.list" | cut -d' ' -f1)"
} > "$receipt"
overall=0
# The standalone engine: one native `jet build` of the assembled hosted unit
# (`assemble.mjs hosted`: every implementation file plus Jetpack/run.jet). The
# binary is the project's build output; on success hosted_binary names it.
hosted_binary=""
build_hosted() {
  unit_record hosted
  local status=0 binary="$scratch/hosted/project/.jet/build/jetpack"
  bounded hosted "${JETPACK_HOSTED_MEM:-20G}" "${JETPACK_HOSTED_SECS:-5400}" "$scratch/hosted/project" "$jet" build src/jetpack.jet || status=1
  node "$bootstrap/locate.mjs" check "$scratch/hosted/jetpack.map.json" "$scratch/hosted.log" > "$scratch/hosted-errors.txt" || true
  if (( status == 0 )) && [[ -x "$binary" ]]; then
    hosted_binary="$binary"
    printf 'hosted-binary: %s\nhosted-binary-sha256: %s\n' "$binary" "$(sha256sum "$binary" | cut -d' ' -f1)" >> "$receipt"
    return 0
  fi
  echo "--- errors by source file ($scratch/hosted-errors.txt)"; grep -A200 '^errors per file:' "$scratch/hosted-errors.txt" || true
  return 1
}
if (( hosted == 1 )); then
  node "$bootstrap/assemble.mjs" hosted
  build_hosted || overall=1
  printf 'exit: %d\n' "$overall" >> "$receipt"
  echo "jetpack-bootstrap receipt: $receipt"
  (( overall == 0 )) && echo "JETPACK HOSTED OK $hosted_binary"
  exit "$overall"
fi
node "$bootstrap/assemble.mjs" check
unit_record check
if (( ${syntax_only:-0} == 1 )); then
  # Grammar only: the pinned compiler's own parser (`jet inspect compiler parse`).
  # Any non-zero status (crash, cap kill) leaves no verdict and fails the check.
  probe_status=0
  ( cd "$scratch/check/project" && systemd-run --user --scope --quiet --expand-environment=no -p MemoryMax=3G -p MemorySwapMax=0 \
    "$jet" inspect compiler parse src/jetpack.jet ) > "$scratch/syntax.parse.json" 2> "$scratch/syntax.parse.err" || probe_status=$?
  if (( probe_status != 0 )); then
    echo "jetpack-bootstrap: parser failed with status $probe_status (3G cap or crash); no verdict ($scratch/syntax.parse.err)" | tee "$scratch/syntax.log"
    overall=1
  else
    node "$bootstrap/locate.mjs" syntax "$scratch/check/jetpack.map.json" < "$scratch/syntax.parse.json" | tee "$scratch/syntax.log"
  fi
  grep -q '^total errors: 0$' "$scratch/syntax.log" || overall=1
  printf 'syntax-log: %s\nexit: %d\n' "$scratch/syntax.log" "$overall" >> "$receipt"
  (( overall == 0 )) && echo "JETPACK SYNTAX OK"
  exit "$overall"
fi
# Full type checks take a machine-wide slot (JETPACK_CHECK_SLOTS, default 2):
# many parallel writers must not run many multi-GB checks at once.
check_slots="${JETPACK_CHECK_SLOTS:-2}"
exec 7>"$HOME/.cache/jet-dev/jetpack-bootstrap/slots/check-wait"
got_slot=""
while [[ -z "$got_slot" ]]; do
  for slot in $(seq 1 "$check_slots"); do
    exec 8>"$HOME/.cache/jet-dev/jetpack-bootstrap/slots/check-$slot"
    if flock -n 8; then got_slot="$slot"; break; fi
    exec 8>&-
  done
  [[ -n "$got_slot" ]] || { flock 7; sleep 5; flock -u 7; }
done
bounded check "$check_mem" "${JETPACK_CHECK_SECS:-1200}" "$scratch/check/project" "$jet" check src/jetpack.jet || overall=1
flock -u 8; exec 8>&-
# Per-file error summary mapped back to source paths (advisory type check).
node "$bootstrap/locate.mjs" check "$scratch/check/jetpack.map.json" "$scratch/check.log" > "$scratch/check-errors.txt" || true
if (( overall != 0 )); then echo "--- errors by source file ($scratch/check-errors.txt)"; grep -A200 '^errors per file:' "$scratch/check-errors.txt" || true; fi

if (( with_tests == 1 && overall == 0 )); then
  node "$bootstrap/assemble.mjs" run
  unit_record run
  # Process claims (Jetpack/CLI/Tests/JetpackBinSupport.jet) run the hosted
  # engine named by JETPACK_BIN, the analog of Cargo's CARGO_BIN_EXE_jetpack.
  # When the unit has such claims and the caller named no binary, the engine
  # is built once per source state: the cache key covers the hosted unit, the
  # compiler, its identity and the Core tree it compiles against.
  if [[ -z "${JETPACK_BIN:-}" ]] && grep -q 'JETPACK_BIN' "$scratch/run/project/src/jetpack.jet"; then
    # The engine is every area whatever this run selected (assemble.mjs refuses
    # a hosted unit with an area selection).
    env -u JETPACK_BOOTSTRAP_AREAS -u JETPACK_BOOTSTRAP_OWN_AREA -u JETPACK_BOOTSTRAP_DEPS_FROM_HEAD node "$bootstrap/assemble.mjs" hosted
    core_sha="$(cd "$JET_TOOLCHAIN_ROOT" && find Core -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1)"
    hosted_key="$(printf '%s\0%s\0%s\0%s' "$(sha256sum "$scratch/hosted/project/src/jetpack.jet" | cut -d' ' -f1)" "$(sha256sum "$jet" | cut -d' ' -f1)" "$JET_COMPILER_IDENTITY" "$core_sha" | sha256sum | cut -c1-16)"
    hosted_cached="$HOME/.cache/jet-dev/jetpack-bootstrap/hosted-bin/$hosted_key/jetpack"
    if [[ ! -x "$hosted_cached" ]]; then
      # A multi-GiB native build: it takes the AOT slot, and a concurrent run
      # that built the same key while this one waited is reused.
      exec 9>"$HOME/.cache/jet-dev/jetpack-bootstrap/slots/aot"
      flock 9
      if [[ ! -x "$hosted_cached" ]] && build_hosted; then
        mkdir -p "$(dirname "$hosted_cached")"
        cp "$hosted_binary" "$hosted_cached.tmp.$$" && mv -f "$hosted_cached.tmp.$$" "$hosted_cached"
      fi
      flock -u 9
      rm -rf "$scratch/hosted/project/.jet/build"
    fi
    if [[ -x "$hosted_cached" ]]; then
      export JETPACK_BIN="$hosted_cached"
    else
      echo "jetpack-bootstrap: the hosted engine did not build, so JETPACK_BIN process claims cannot run" >&2
      overall=1
    fi
  fi
  if [[ -n "${JETPACK_BIN:-}" ]]; then
    printf 'jetpack-bin: %s\njetpack-bin-sha256: %s\n' "$JETPACK_BIN" "$(sha256sum "$JETPACK_BIN" | cut -d' ' -f1)" >> "$receipt"
  fi
  # Collect-all: a failing claim stops the driver with E3001 (exit 70) right
  # after its predecessors' `pass` lines; it is recorded as failed and the
  # driver resumes at the next claim (JETPACK_CLAIM_FROM). Any other stop
  # (ICE, timeout, memory cap) can't be attributed to a claim and ends the tier.
  # The default-tier image (`jet run`'s options: `--optimize=none`) is built
  # once and every segment runs it, so a failing claim costs one restart, not
  # one recompile (#6293).
  # Both tiers' claim results are classified by known-claims.mjs against the
  # `jetpack-claim:<Area>/<claim>` rows of Tools/testkit/known-failing.tsv: a
  # listed failure is KNOWN-FAILING, a listed pass is UNEXPECTED-PASS (fails).
  expected="$(node -e 'console.log(require(process.argv[1]).tests.length)' "$scratch/run/jetpack.map.json")"
  from=0 label=run run_failed=0 run_passed=0 run_complete=1
  run_results="$scratch/run-results.txt"
  : > "$run_results"
  run_binary="$scratch/run/project/.jet/build/jetpack" run_built=0
  if bounded run-build "$check_mem" 1800 "$scratch/run/project" "$jet" build --optimize=none src/jetpack.jet && [[ -x "$run_binary" ]]; then
    run_built=1
  else
    echo "jetpack-bootstrap: default tier stopped: the default-tier image did not build" >&2
    run_complete=0
  fi
  while (( run_built == 1 )); do
    run_status=0
    JETPACK_CLAIM_FROM="$from" bounded "$label" "$check_mem" 1800 "$scratch/run/project" "$run_binary" || run_status=$?
    grep '^pass ' "$scratch/$label.log" >> "$run_results" || true
    segment="$(grep -c '^pass ' "$scratch/$label.log" || true)"
    run_passed=$(( run_passed + segment ))
    if (( run_status == 0 )) && grep -qx 'jetpack-bootstrap: claims done' "$scratch/$label.log"; then break; fi
    stopped=$(( from + segment ))
    if (( run_status != 70 || stopped >= expected )); then
      echo "jetpack-bootstrap: default tier stopped (exit $run_status) without a claim to attribute it to" >&2
      run_complete=0
      break
    fi
    echo "fail $(node -e 'console.log(require(process.argv[1]).tests[Number(process.argv[2])])' "$scratch/run/jetpack.map.json" "$stopped")" | tee -a "$run_results"
    run_failed=$(( run_failed + 1 ))
    from=$(( stopped + 1 ))
    (( from < expected )) || break
    label="run-from-$from"
  done
  echo "jetpack-bootstrap: default tier: $run_passed of $expected claims passed, $run_failed failed"
  (( run_complete == 1 && run_passed + run_failed == expected )) || overall=1
  node "$bootstrap/known-claims.mjs" run "$scratch/run/jetpack.map.json" "$run_results" || overall=1
  rm -rf "$scratch/run/project/.jet/build"

  node "$bootstrap/assemble.mjs" aot
  unit_record aot
  # One AOT build at a time machine-wide for this stream (~1 GiB+ per build).
  exec 9>"$HOME/.cache/jet-dev/jetpack-bootstrap/slots/aot"
  flock 9
  aot_status=0
  bounded aot "$aot_mem" 1800 "$scratch/aot/project" "$jet" test src/jetpack.jet || aot_status=$?
  flock -u 9
  rm -rf "$scratch/aot/project/.jet/build"
  # `jet test` exits 1 when a claim fails; any other stop, or no summary line,
  # is not a claim result.
  if (( aot_status <= 1 )) && grep -Eq '^[0-9]+ passed, [0-9]+ failed' "$scratch/aot.log"; then
    node "$bootstrap/known-claims.mjs" aot "$scratch/aot/jetpack.map.json" "$scratch/aot.log" || overall=1
  else
    echo "jetpack-bootstrap: AOT tier stopped (exit $aot_status) before its claim summary" >&2
    overall=1
  fi
fi

printf 'exit: %d\n' "$overall" >> "$receipt"
echo "jetpack-bootstrap receipt: $receipt"
if (( overall == 0 )); then
  echo "JETPACK CHECK OK$([[ $with_tests == 1 ]] && echo ' (check, default run, AOT test)')"
fi
exit "$overall"
