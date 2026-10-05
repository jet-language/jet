#!/usr/bin/env bash
# Measure one jetc0 compile of hello: wall time, RSS every second, perf every 10 s.
set -u
L=$HOME/.cache/jet-dev
D=$L/loop
OUT=${1:-$L/stage1/measure-hello}
rm -rf "$OUT"; mkdir -p "$OUT"
export JETC0_KEEP=$D
. "$L/stage1/lib.sh"
( while sleep 1; do p=$(pgrep -f "^$D/jetc0\$" | head -1); [ -n "$p" ] || continue; echo "$(date +%s.%N | cut -c1-14) $(awk '/VmRSS/{print $2}' /proc/$p/status 2>/dev/null)"; done ) > "$OUT/rss.txt" &
mon=$!
t0=$(date +%s.%N)
STAGE1_SAMPLE_SECS=10 STAGE1_JETC_MEM=24G STAGE1_JETC_TIMEOUT=600 \
  jetc "$D/jetc0" runner "$D/hello" main.jet "$OUT/hello.raw.rs" "$OUT/hello.receipt" "$OUT/jetc0.log"
rc=$?
t1=$(date +%s.%N)
kill $mon 2>/dev/null
echo "rc=$rc wall=$(python3 -c "print(round($t1-$t0,1))")s" > "$OUT/summary.txt"
