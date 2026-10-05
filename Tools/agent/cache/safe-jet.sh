#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
# Memory-safe jet invocation for agents.
# usage: safe-jet.sh <jet args...>          e.g. safe-jet.sh run Examples/features/x.jet
# Uses the frozen snapshot binary in $JET (default: jet-current symlink),
# at most 3 concurrent machine-wide (slot locks, first free slot wins),
# 6 GB hard cap (override with SAFE_JET_MEM), 5 min timeout (override with SAFE_JET_TIMEOUT).
set -u
# Failed transient scopes linger and collide with reused PIDs ("already loaded"): clear them first.
systemctl --user reset-failed 2>/dev/null
ulimit -c 0  # no core dumps / crash reports from agent probes
jet=${JET:-$HOME/.cache/jet-dev/scratch/jet-current}
case "$(basename "$jet")" in safe-jet.sh) echo "safe-jet.sh refuses to wrap itself or a script (JET=$jet); point JET at a jet binary" >&2; exit 64;; esac
slots=$HOME/.cache/jet-dev/jet-slots
limit=${SAFE_JET_TIMEOUT:-300}
mkdir -p "$slots"
while :; do
  for i in 1 2 3; do
    exec 9>"$slots/$i"
    if flock -n 9; then
      exec systemd-run --user --slice=jetwork.slice --scope -q --unit="jw-$(basename "$0" .sh)-$$-$(date +%s%N)" -p MemoryMax=${SAFE_JET_MEM:-6G} -p MemorySwapMax=0 \
        env TMPDIR=$HOME/.cache/jet-dev/scratch JET_NIX_TMP_CLEANED=1 timeout "$limit" $JET_REPO/Tools/agent/jet-env "$jet" "$@"
    fi
    exec 9>&-
  done
  sleep 0.5
done
