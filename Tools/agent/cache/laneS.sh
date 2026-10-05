#!/usr/bin/env bash
# Small-job lane: up to 4 concurrent jobs, each capped at $LANES_MEM (default 3G, max 8G; no swap).
# usage: laneS.sh <command...>
set -u
systemctl --user reset-failed 2>/dev/null
d=$HOME/.cache/jet-dev/laneS; mkdir -p "$d"
while :; do
  for i in 1 2 3 4; do
    if flock -n "$d/$i" true 2>/dev/null; then
      mem=${LANES_MEM:-3G}; case "$mem" in 1G|2G|3G|4G|5G|6G|7G|8G) ;; *) echo "laneS: LANES_MEM must be 1G..8G (got $mem)" >&2; exit 2;; esac
      exec flock -o "$d/$i" systemd-run --user --slice=jetwork.slice --scope -q --unit="jw-laneS-$$-$(date +%s%N)" -p MemoryMax="$mem" -p MemorySwapMax=0 bash -c 'ulimit -c 0; exec "$@"' laneS "$@"
    fi
  done
  sleep 1
done
