#!/usr/bin/env bash
# Attach only: never starts or restarts the compiler. `jet perf attach` samples
# the running jet, and the processes it started, from outside for the window
# given (`500ms`, `20s`; 20 seconds when none is).
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 || ! $1 =~ ^[1-9][0-9]*$ || ! ${2:-20s} =~ ^[1-9][0-9]*(ms|s)$ ]]; then
  echo 'usage: live.sh <pid> [<window>, as 500ms or 20s]' >&2; exit 2
fi
pid=$1
window=${2:-20s}
kill -0 "$pid"
here=$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")
jet=${JET:-jet}
. "$here/../../Agent/roots.sh"
out=${JET_PROFILE_OUT:-$JET_SCRATCH_ROOT/sol/SolJetcProfile/jetc-$pid-$(date +%Y%m%d-%H%M%S)}
mkdir -p "$out"
echo "Sampling PID $pid for $window; output: $out" >&2
$jet perf attach "$pid" --for="$window" --out "$out/attach.jettrace"
$jet perf view "$out/attach.jettrace" > "$out/view.txt"
cat "$out/view.txt"
echo "Artifacts: $out (attach.jettrace, view.txt)"
