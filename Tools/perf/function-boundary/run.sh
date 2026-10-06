#!/usr/bin/env bash
# Function-boundary cells (#4566): build each cell's Jet (O1 = `jet build`,
# O2 = `jet build --release`), C and Rust programs, check every output against
# the cell's expected.out, then time each binary pinned to one CPU and print
# median and spread with Jet/Rust and Jet/C ratios.
#
# usage: Tools/perf/function-boundary/run.sh [--jet PATH] [--runs N] [--cpu N]
#            [--out DIR] [--levels O1,O2] [cell ...]
# Defaults: --jet target-rel4/release/jet, --runs 7, --cpu 31,
#           --out ~/.cache/jet-dev/scratch/function-boundary, all cells.
# Builds and timings wait until MemAvailable >= FB_MEM_GATE_GB (default 30 GiB,
# the AGENTS memory rule); Jet builds run in the capped small lane (laneS.sh)
# with FB_LANE_MEM (default 8G), one at a time.
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
jet=$root/target-rel4/release/jet
runs=7
cpu=31
out=$HOME/.cache/jet-dev/scratch/function-boundary
levels=O1,O2
cells=()
while [ $# -gt 0 ]; do
  case "$1" in
    --jet) jet=$2; shift 2 ;;
    --runs) runs=$2; shift 2 ;;
    --cpu) cpu=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    --levels) levels=$2; shift 2 ;;
    -*) echo "unknown option $1" >&2; exit 64 ;;
    *) cells+=("$1"); shift ;;
  esac
done
manifest=$here/cells.tsv
if [ ${#cells[@]} -eq 0 ]; then
  while IFS=$'\t' read -r cell _; do
    case "$cell" in ''|\#*|cell) continue ;; esac
    cells+=("$cell")
  done < "$manifest"
fi
mkdir -p "$out"
env_run() { (cd "$root" && Tools/agent/jet-env "$@"); }
mem_gate() {
  until [ "$(awk '/MemAvailable/ {print int($2/1048576)}' /proc/meminfo)" -ge "${FB_MEM_GATE_GB:-30}" ]; do sleep 30; done
}
field() { awk -F'\t' -v c="$1" -v k="$2" '$1 == c { print $k }' "$manifest"; }

build() {
  local cell=$1 dir=$here/$1 bin=$out/bin
  mkdir -p "$bin"
  # The devshell's cc wrapper drops -march=native unless told otherwise; Jet
  # and the Rust peer both build for the host CPU, so C must too.
  [ -f "$dir/main.c" ] && [ ! -x "$bin/$cell.c" ] && { mem_gate; env_run env NIX_ENFORCE_NO_NATIVE=0 clang -O2 -march=native -o "$bin/$cell.c" "$dir/main.c"; }
  [ -f "$dir/main.rs" ] && [ ! -x "$bin/$cell.rust" ] && { mem_gate; env_run rustc --edition 2021 -C opt-level=3 -C codegen-units=1 -C target-cpu=native -o "$bin/$cell.rust" "$dir/main.rs"; }
  [ -f "$dir/main.jet" ] || return 0
  local level flag work
  for level in ${levels//,/ }; do
    [ -x "$bin/$cell.jet-$level" ] && continue
    flag=(); [ "$level" = O2 ] && flag=(--release)
    work=$out/work/$cell-$level
    rm -rf "$work"; mkdir -p "$work"; cp "$dir/main.jet" "$work/main.jet"
    mem_gate
    (cd "$root" && LANES_MEM=${FB_LANE_MEM:-8G} bash Tools/agent/cache/laneS.sh Tools/agent/jet-env "$jet" build --quiet "${flag[@]}" "$work/main.jet") > "$out/$cell.$level.build.log" 2>&1
    if [ -x "$work/.jet/build/main" ]; then
      cp "$work/.jet/build/main" "$bin/$cell.jet-$level"
      cp "$work/.jet/build/main.rs" "$out/$cell.$level.generated.rs"
    else
      echo "$cell $level: build failed, see $out/$cell.$level.build.log" >&2
    fi
  done
}

# Prints "median min max" in milliseconds over $runs pinned runs, or "fail".
measure() {
  local exe=$1 n=$2 expected=$3 i start end got samples=()
  got=$(taskset -c "$cpu" "$exe" "$n" 2>/dev/null)
  [ "$got" = "$(cat "$expected")" ] || { echo fail; return; }
  for ((i = 0; i < runs; i++)); do
    start=$EPOCHREALTIME
    taskset -c "$cpu" "$exe" "$n" > /dev/null 2>&1
    end=$EPOCHREALTIME
    samples+=("$(awk -v s="$start" -v e="$end" 'BEGIN { printf "%.3f", (e - s) * 1000 }')")
  done
  printf '%s\n' "${samples[@]}" | sort -g | awk '{ v[NR] = $1 } END { printf "%.1f %.1f %.1f", v[int((NR + 1) / 2)], v[1], v[NR] }'
}

printf 'cell\tpair\timpl\tmedian_ms\tmin_ms\tmax_ms\tvs_rust\tvs_c\n'
for cell in "${cells[@]}"; do
  build "$cell"
  n=$(field "$cell" 3)
  pair=$(field "$cell" 2)
  expected=$here/$cell/expected.out
  declare -A med=()
  for impl in c rust ${levels//,/ }; do
    exe=$out/bin/$cell.$impl
    case "$impl" in O*) exe=$out/bin/$cell.jet-$impl ;; esac
    if [ ! -x "$exe" ]; then
      printf '%s\t%s\t%s\tunavailable\t-\t-\t-\t-\n' "$cell" "$pair" "${impl/O/jet-O}"
      continue
    fi
    mem_gate
    read -r m lo hi <<< "$(measure "$exe" "$n" "$expected")"
    if [ "$m" = fail ]; then
      printf '%s\t%s\t%s\twrong-output\t-\t-\t-\t-\n' "$cell" "$pair" "${impl/O/jet-O}"
      continue
    fi
    med[$impl]=$m
    vs_rust=-; vs_c=-
    case "$impl" in
      O*)
        [ -n "${med[rust]:-}" ] && vs_rust=$(awk -v a="$m" -v b="${med[rust]}" 'BEGIN { printf "%.2f", a / b }')
        [ -n "${med[c]:-}" ] && vs_c=$(awk -v a="$m" -v b="${med[c]}" 'BEGIN { printf "%.2f", a / b }')
        ;;
    esac
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$cell" "$pair" "${impl/O/jet-O}" "$m" "$lo" "$hi" "$vs_rust" "$vs_c"
  done
  unset med
done
