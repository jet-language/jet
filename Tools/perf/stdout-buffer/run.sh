#!/usr/bin/env bash
# Stdout buffer receipts, before and after: fd-1 write syscalls and wall time
# for a Jet program printing N lines (`lines`) or writing N one-byte strings
# (`chars`) to a file, a pipe and a terminal, at N = 1M and 2M, beside
# python3 (buffered) and python3 -u (unbuffered) doing the same.
#
# usage: Tools/perf/stdout-buffer/run.sh [--jet PATH] [--before-jet PATH]
#                                        [--runs N] [--out DIR]
# Defaults: --jet target-rel4/release/jet, --runs 3 (best wall time kept),
#           --out $JET_SCRATCH_ROOT/stdout-buffer.
# Needs strace, script (util-linux) and python3. The Jet program is built
# once with `jet build -O2`; run the script in the heavy slot.
#
# Programs, one row each per (N, target, mode):
#   jet         the buffered stdout under test (after).
#   jet-flush   the same binary flushing after every print or write: the
#               per-write flush stdout had before it was buffered (before).
#   jet-before  with --before-jet, lines.jet built by that toolchain: the
#               runtime from before the change, measured as it shipped.
#   python3, python3-u  for comparison.
#
# What passes, per (N, target, mode) for Jet:
#   file, pipe   writes <= ceil(bytes / 8192) + 1: the 8192-byte buffer
#                drains only when full and at exit.
#   tty, lines   writes <= N + 1: a terminal is line buffered, one write
#                per LF.
#   tty, chars   writes <= ceil(bytes / 8192) + 1: no LF until the end.
#   file, pipe   writes below jet-flush's (and jet-before's) writes.
#   file, pipe   best wall time <= jet-flush's (and jet-before's) and
#                <= python3's (buffered) best wall time.
# Terminal timings and both python3 columns are printed for comparison only.
# The script prints one row per measurement, then PASS or FAIL per check,
# and exits 1 when any check fails. results.tsv keeps every row.
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
jet=$root/target-rel4/release/jet
before_jet=
runs=3
. "$root/Tools/agent/roots.sh"
out=$JET_SCRATCH_ROOT/stdout-buffer
while [ $# -gt 0 ]; do
  case "$1" in
    --jet) jet=$2; shift 2 ;;
    --before-jet) before_jet=$2; shift 2 ;;
    --runs) runs=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 64 ;;
  esac
done
for tool in strace script python3; do
  command -v "$tool" > /dev/null || { echo "missing $tool" >&2; exit 69; }
done
mkdir -p "$out"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

(cd "$here" && "$jet" build --quiet -O2 --output-dir="$out/bin" lines.jet) || { echo "jet build failed" >&2; exit 1; }
bin=$out/bin/lines
programs=(jet jet-flush)
if [ -n "$before_jet" ]; then
  (cd "$here" && "$before_jet" build --quiet -O2 --output-dir="$out/before" lines.jet) || { echo "before jet build failed" >&2; exit 1; }
  programs+=(jet-before)
fi
programs+=(python3 python3-u)
cat > "$work/lines.py" <<'PY'
import sys
n = int(sys.argv[2])
if sys.argv[1] == "chars":
    w = sys.stdout.write
    for _ in range(n):
        w("x")
    w("\n")
else:
    for i in range(n):
        print(f"line {i}")
PY

# The command, as words, for one program and one mode and count; `run_target`
# runs it to a target.
program() {
  case $1 in
    jet) printf '%s\n' "$bin" "$2" "$3" ;;
    jet-flush) printf '%s\n' "$bin" "$2" "$3" flush ;;
    jet-before) printf '%s\n' "$out/before/lines" "$2" "$3" ;;
    python3) printf '%s\n' python3 "$work/lines.py" "$2" "$3" ;;
    python3-u) printf '%s\n' python3 -u "$work/lines.py" "$2" "$3" ;;
  esac
}

# run_target TARGET STRACE_FILE CMD... : run CMD with stdout on TARGET; with a
# non-empty STRACE_FILE, under strace counting only write and writev.
run_target() {
  local target=$1 trace=$2; shift 2
  local prefix=()
  [ -n "$trace" ] && prefix=(strace -f -qq -e trace=write,writev -o "$trace")
  case $target in
    file) "${prefix[@]}" "$@" > "$work/out" ;;
    pipe) "${prefix[@]}" "$@" | cat > "$work/out" ;;
    tty) script -qec "$(printf '%q ' "${prefix[@]}" "$@")" /dev/null > "$work/out" ;;
  esac
}

fd1_writes() { awk '/(^|[ \t])(write|writev)\(1,/ {n++} END {print n+0}' "$1"; }

failed=0
check() { # name ok detail
  if [ "$2" = 1 ]; then echo "PASS $1: $3"; else echo "FAIL $1: $3"; failed=1; fi
}

results=$out/results.tsv
printf 'n\ttarget\tmode\tprogram\twrites\tbytes\tbest_ms\n' > "$results"
printf '%-8s %-5s %-6s %-10s %10s %10s %8s\n' n target mode program writes bytes best_ms
for n in 1000000 2000000; do
  for target in file pipe tty; do
    for mode in lines chars; do
      declare -A best=()
      declare -A count=()
      for name in "${programs[@]}"; do
        mapfile -t cmd < <(program "$name" "$mode" "$n")
        ms_best=
        for _ in $(seq "$runs"); do
          start=$(date +%s%N)
          run_target "$target" "" "${cmd[@]}"
          ms=$(( ($(date +%s%N) - start) / 1000000 ))
          if [ -z "$ms_best" ] || [ "$ms" -lt "$ms_best" ]; then ms_best=$ms; fi
        done
        run_target "$target" "$work/trace" "${cmd[@]}"
        writes=$(fd1_writes "$work/trace")
        # The program's own bytes: N lines of `line i` + LF, or N bytes + LF.
        if [ "$mode" = lines ]; then
          bytes=$(python3 -c "import sys; n=int(sys.argv[1]); print(sum(len(f'line {i}\n') for i in range(n)))" "$n")
        else
          bytes=$((n + 1))
        fi
        best[$name]=$ms_best
        count[$name]=$writes
        printf '%-8s %-5s %-6s %-10s %10s %10s %8s\n' "$n" "$target" "$mode" "$name" "$writes" "$bytes" "$ms_best"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$n" "$target" "$mode" "$name" "$writes" "$bytes" "$ms_best" >> "$results"
        if [ "$name" = jet ]; then
          full=$(( (bytes + 8191) / 8192 + 1 ))
          if [ "$target" = tty ] && [ "$mode" = lines ]; then limit=$((n + 1)); else limit=$full; fi
          check "jet $target $mode $n writes" "$([ "$writes" -le "$limit" ] && echo 1 || echo 0)" "$writes <= $limit"
        fi
      done
      if [ "$target" != tty ]; then
        for before in jet-flush jet-before; do
          [ -n "${count[$before]:-}" ] || continue
          check "jet $target $mode $n writes vs $before" "$([ "${count[jet]}" -lt "${count[$before]}" ] && echo 1 || echo 0)" "jet ${count[jet]} < $before ${count[$before]}"
          check "jet $target $mode $n time vs $before" "$([ "${best[jet]}" -le "${best[$before]}" ] && echo 1 || echo 0)" "jet ${best[jet]} ms <= $before ${best[$before]} ms"
        done
        check "jet $target $mode $n time" "$([ "${best[jet]}" -le "${best[python3]}" ] && echo 1 || echo 0)" "jet ${best[jet]} ms <= python3 ${best[python3]} ms"
      fi
      unset best count
    done
  done
done
echo "results: $results"
exit $failed
