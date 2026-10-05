#!/usr/bin/env bash
# Main's memory guard (03:50): protects the orchestrator (omp) from a global OOM.
# Every 2 s: if MemAvailable < 7 GiB, SIGKILL the largest-RSS process among
# compiler/build/test jobs (jetc0, jet test bins, rustc, cargo, jet, node tools,
# python runners). Never touches omp, the desktop, the browser or tower-serve.
LOG=$HOME/.cache/jet-dev/mem-guard.log
LIMIT_KB=$((7 * 1024 * 1024))
while true; do
  avail=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)
  if [ "$avail" -lt "$LIMIT_KB" ]; then
    victim=$(ps -eo pid=,rss=,comm=,args= --sort=-rss | awk '
      $3 ~ /^(jetc0|jetc1|rustc|cargo|jet|jet-[a-z0-9-]+|python[0-9.]*|ld|cc|lld|perf)$/ { print $1, $2, $3; exit }
      $0 ~ /deps\/jet-[0-9a-f]+/ { print $1, $2, $3; exit }')
    if [ -n "$victim" ]; then
      set -- $victim
      kill -9 "$1" 2>/dev/null
      echo "$(date '+%F %T') avail=${avail}kB killed pid=$1 rss=${2}kB comm=$3" >> "$LOG"
    else
      echo "$(date '+%F %T') avail=${avail}kB no eligible victim" >> "$LOG"
    fi
    sleep 5
  fi
  sleep 2
done
