#!/usr/bin/env bash
# capped.sh <MemoryMax e.g. 16G> <command...>: run a heavy job in its own transient systemd scope
# with a hard memory cap and no swap, so a runaway job is OOM-killed alone (never the harness).
MAX=${1:?}; shift
exec systemd-run --user --scope --quiet --collect -p MemoryMax="$MAX" -p MemorySwapMax=0 -p CPUWeight=1 -p IOWeight=1 -- nice -n 19 ionice -c3 ${CAP_CPUS:+taskset -c $CAP_CPUS} "$@"
