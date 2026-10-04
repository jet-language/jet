#!/usr/bin/env bash
# Attach only: never starts or restarts the compiler.
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 || ! $1 =~ ^[1-9][0-9]*$ || ! ${2:-20} =~ ^[1-9][0-9]*$ ]]; then
  echo 'usage: live.sh <pid> [seconds (default 20)]' >&2; exit 2
fi
pid=$1; seconds=${2:-20}
kill -0 "$pid"
here=$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")
perf=${PERF:-}
if [[ -z $perf ]]; then
  perf=$(command -v perf || true)
  if [[ -z $perf ]]; then
    for candidate in /nix/store/*-perf-linux-*/bin/perf; do
      [[ -x $candidate ]] && perf=$candidate && break
    done
  fi
fi
[[ -n $perf && -x $perf ]] || { echo 'Set PERF to a working perf binary.' >&2; exit 1; }
map=${JET_SOURCE_MAP:-$HOME/.cache/jet-dev/stage1/run-loop6g/selfcheck/compiler.map.json}
repo=${JET_REPO:-$(readlink -f "$here/../../..")}
out=${JET_PROFILE_OUT:-$HOME/.cache/jet-dev/scratch/sol/SolJetcProfile/jetc-$pid-$(date +%Y%m%d-%H%M%S)}
mkdir -p "$out"
mode=${JET_CALL_GRAPH:-auto}
case $mode in auto|fp|dwarf) ;; *) echo 'JET_CALL_GRAPH must be auto, fp, or dwarf' >&2; exit 2;; esac
record() {
  local graph=$1
  echo "Sampling PID $pid for ${seconds}s ($graph); output: $out" >&2
  "$perf" record -m 8 -e cycles:u -F 99 -p "$pid" -g --call-graph="$graph" -o "$out/perf.data" -- sleep "$seconds"
  # Inline expansion starts addr2line on the huge generated Rust image; symbol
  # callchains plus the Jet declaration map are sufficient and memory-bounded.
  "$perf" script -i "$out/perf.data" --demangle --no-inline --max-stack 256 -F comm,pid,tid,time,event,ip,sym,dso > "$out/perf.script"
  node "$here/profile.mjs" analyze "$out/perf.script" "$map" "$repo" "$out" "$graph"
}
if [[ $mode == dwarf ]]; then
  record dwarf,16384
else
  record fp
  if [[ $mode == auto ]] && node "$here/profile.mjs" needs-dwarf "$out/summary.json"; then
    echo 'Frame-pointer stacks are mostly truncated/unresolved; collecting a fresh DWARF window.' >&2
    mv "$out/perf.data" "$out/perf.fp.data"
    mv "$out/perf.script" "$out/perf.fp.script"
    mv "$out/summary.json" "$out/summary.fp.json"
    record dwarf,16384
  fi
fi
cat "$out/top.txt" "$out/phases.txt"
echo "Artifacts: $out (stacks.folded, flamegraph.svg, summary.json, perf.data, perf.script)"
