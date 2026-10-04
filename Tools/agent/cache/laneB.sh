#!/usr/bin/env bash
# Shared heavy lane for workers: two slots machine-wide, each capped.
# usage: laneB.sh <GB (<=12)> <command...>
set -u
# Failed transient scopes linger and collide with reused PIDs ("already loaded"): clear them first.
systemctl --user reset-failed 2>/dev/null
gb=${1:?GB}; shift
[ "$gb" -le 12 ] || { echo "laneB: cap is 12 GB" >&2; exit 2; }
run() { exec flock -o "$1" systemd-run --user --slice=jetwork.slice --scope -q --unit="jw-$(basename "$0" .sh)-$$-$(date +%s%N)" -p MemoryMax=${gb}G -p MemorySwapMax=0 bash -c 'ulimit -c 0; exec "$@"' laneB "${@:2}"; }
slot1=$HOME/.cache/jet-dev/unitcheck/lock
slot2=$HOME/.cache/jet-dev/laneB2.lock
while :; do
  for s in "$slot2" "$slot1"; do
    if flock -n "$s" true 2>/dev/null; then run "$s" "$@"; fi
  done
  sleep 2
done
