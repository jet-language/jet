#!/usr/bin/env bash
# Attach only: never starts or restarts the compiler. `jet perf attach` samples
# the running jet, and the processes it started, from outside for a second.
set -euo pipefail
if [[ $# -ne 1 || ! $1 =~ ^[1-9][0-9]*$ ]]; then
  echo 'usage: live.sh <pid>' >&2; exit 2
fi
pid=$1
kill -0 "$pid"
here=$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")
jet=${JET:-jet}
. "$here/../../Agent/roots.sh"
out=${JET_PROFILE_OUT:-$JET_SCRATCH_ROOT/sol/SolJetcProfile/jetc-$pid-$(date +%Y%m%d-%H%M%S)}
mkdir -p "$out"
echo "Sampling PID $pid for 1s; output: $out" >&2
$jet perf attach "$pid" --out "$out/attach.jettrace"
$jet perf view "$out/attach.jettrace" > "$out/view.txt"
cat "$out/view.txt"
echo "Artifacts: $out (attach.jettrace, view.txt)"
