#!/usr/bin/env bash
# Where a cold `jet run` spends its CPU: the user's own code, or the fixed
# cost around it (sources, load, registration, finalize, lowering, link).
#
#   Tools/Perf/ColdAttribution/run.sh [--runs N] [--keep DIR] <program.jet> [jet run args...]
#
# Each round makes two cold runs, each with a fresh, empty JET_STORE_DIR:
# one untraced and timed (user, sys and wall seconds), then one traced
# (`jet run --trace=FILE`). The trace attributes CPU (the cpu_ns of each span's end)
# to the user's code: the parse spans of the program's own source files and
# the sema.module spans of its own modules. A span counts as the user's when
# the file it names exists under the program's directory; Core's files and
# modules (Core/..., core, Prelude/...) never do. Everything else is fixed
# cost, shown per top-level phase.
#
# Prints every round, then the medians: untraced user CPU, the user code's
# CPU from the trace, its share of the untraced user CPU, and each top-level
# phase's CPU. The jet is $JET, else the one on PATH. --keep DIR keeps the
# traces and timings there.
set -euo pipefail

runs=5
keep=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) runs="$2"; shift 2 ;;
    --keep) keep="$2"; shift 2 ;;
    -h|--help) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    *) break ;;
  esac
done
[[ $# -ge 1 ]] || { echo "usage: run.sh [--runs N] [--keep DIR] <program.jet> [jet run args...]" >&2; exit 64; }
program="$1"; shift
[[ -f "$program" ]] || { echo "run.sh: no file $program" >&2; exit 66; }
jet="${JET:-$(command -v jet)}"
dir="$(cd "$(dirname "$program")" && pwd)"
file="$(basename "$program")"

work="${keep:-$(mktemp -d)}"
mkdir -p "$work"
[[ -n "$keep" ]] || trap 'rm -rf "$work"' EXIT

TIMEFORMAT='%3U %3S %3R'
for ((i = 1; i <= runs; i++)); do
  rm -rf "$work/store" "$work/tstore"
  { time JET_STORE_DIR="$work/store" "$jet" run "$dir/$file" "$@" > "$work/out$i.txt" 2>&1; } 2> "$work/time$i.txt"
  JET_STORE_DIR="$work/tstore" "$jet" run --trace="$work/trace$i.json" "$dir/$file" "$@" > "$work/tout$i.txt" 2>&1
  rm -rf "$work/store" "$work/tstore"
  # The names the trace's parse and sema.module spans carry, and which of
  # them are the user's: a file of that name (with .jet added for a
  # module) under the program's directory.
  awk -F'"' '/"ph":"B"/ && ($8 == "parse" || $8 == "sema.module") { print $4 }' "$work/trace$i.json" | sort -u > "$work/names$i.txt"
  : > "$work/user$i.txt"
  while IFS= read -r name; do
    if [[ -f "$dir/$name" || -f "$dir/$name.jet" ]]; then printf '%s\n' "$name" >> "$work/user$i.txt"; fi
  done < "$work/names$i.txt"
  # One row per round: untraced user, sys, wall; the user code's traced CPU;
  # then phase=cpu for each top-level span (by category), all in ms.
  awk -v timing="$(< "$work/time$i.txt")" '
    FNR == NR { user[$0] = 1; next }
    {
      if ($0 !~ /"ph":"[BE]"/) next
      name = $0; sub(/.*"name":"/, "", name); sub(/".*/, "", name)
      cat = name
      if ($0 ~ /"cat":"/) { cat = $0; sub(/.*"cat":"/, "", cat); sub(/".*/, "", cat) }
      tid = $0; sub(/.*"tid":/, "", tid); sub(/[,}].*/, "", tid)
      if ($0 ~ /"ph":"B"/) {
        d = ++depth[tid]
        mine = ((cat == "parse" || cat == "sema.module") && (name in user))
        under[tid, d] = mine || (d > 1 && under[tid, d - 1])
        kind[tid, d] = cat
        own[tid, d] = mine
        next
      }
      d = depth[tid]
      if (d < 1) next
      cpu = 0
      if ($0 ~ /"cpu_ns":/) { cpu = $0; sub(/.*"cpu_ns":/, "", cpu); sub(/[,}].*/, "", cpu); cpu += 0 }
      # A user span inside another user span is already counted.
      if (own[tid, d] && !(d > 1 && under[tid, d - 1])) usercpu += cpu
      if (d == 1) { if (!(kind[tid, d] in phase)) order[++phases] = kind[tid, d]; phase[kind[tid, d]] += cpu }
      depth[tid] = d - 1
    }
    END {
      split(timing, t, " ")
      line = sprintf("untraced_user=%.1f untraced_sys=%.1f untraced_wall=%.1f user_code=%.3f", t[1] * 1000, t[2] * 1000, t[3] * 1000, usercpu / 1e6)
      for (k = 1; k <= phases; k++) line = line sprintf(" %s=%.3f", order[k], phase[order[k]] / 1e6)
      print line
    }' "$work/user$i.txt" "$work/trace$i.json" > "$work/row$i.txt"
  printf 'round %d: user code %s; %s\n' "$i" "$(paste -sd, "$work/user$i.txt")" "$(< "$work/row$i.txt")"
done

# Medians of every column over the rounds.
for ((i = 1; i <= runs; i++)); do awk 1 "$work/row$i.txt"; done | awk '
  {
    for (k = 1; k <= NF; k++) {
      split($k, kv, "=")
      if (!(kv[1] in count)) order[++keys] = kv[1]
      values[kv[1], ++count[kv[1]]] = kv[2] + 0
    }
  }
  function median(key,   n, a, i, j, x) {
    n = count[key]
    for (i = 1; i <= n; i++) a[i] = values[key, i]
    for (i = 2; i <= n; i++) { x = a[i]; for (j = i - 1; j >= 1 && a[j] > x; j--) a[j + 1] = a[j]; a[j + 1] = x }
    return n % 2 ? a[(n + 1) / 2] : (a[n / 2] + a[n / 2 + 1]) / 2
  }
  END {
    user = median("untraced_user")
    code = median("user_code")
    printf "medians over %d cold rounds (ms):\n", NR
    printf "  untraced user CPU %.1f, sys %.1f, wall %.1f\n", user, median("untraced_sys"), median("untraced_wall")
    printf "  user code (traced) %.3f = %.1f%% of untraced user CPU (%s half)\n", code, (user > 0 ? 100 * code / user : 0), (2 * code > user ? "more than" : "not more than")
    printf "  traced CPU by top-level phase:\n"
    for (k = 1; k <= keys; k++) if (order[k] !~ /^untraced_|^user_code$/) printf "    %-28s %9.3f\n", order[k], median(order[k])
  }'
