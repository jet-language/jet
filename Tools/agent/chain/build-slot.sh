#!/usr/bin/env bash
# build-slot.sh <command...> : run a heavy build (chain, seed build, runtime pack, corpus gate, measurement) in one
# of three machine-wide build slots, split by PHYSICAL core (cpu N and N+16 are SMT siblings): slot A = cores 4-9
# (CPUs 4-9,20-25), slot B = cores 10-15 (CPUs 10-15,26-31), slot C = both (only while MemAvailable > 26 GB).
# A and B never share a physical core, so a measurement in one isn't skewed by its neighbour's SMT sibling.
# Cores 0-3 (CPUs 0-3,16-19) stay free for the owner's desktop. A slot opens only while MemAvailable > 20 GB and
# the I/O pressure (avg10) is below 85% or the NVMe disks were under 50% busy over the last wait (a task parked in
# iowait can hold PSI near 90% while the disks sit idle), so the
# owner's machine keeps headroom. Exports SLOT_CPUS (and CHAIN_CPUS/CAP_CPUS) to the command.
# Waiters are served in ticket order: each waiter holds a ticket in $JET_RUN_ROOT/slot-queue and only the first live
# ticket may take a free slot. Classes: SLOT_PRIORITY=1 (Main's use only) first, then SLOT_SMALL=1 (a job that needs
# under ~2 min and under ~3 GB: one runtime pack, a seed check of a few members, a handful of examples), then the rest,
# each class first come, first served. Every job appends a row to $JET_RUN_ROOT/slot-ledger.tsv.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
Q=$JET_RUN_ROOT/slot-queue
mkdir -p "$Q"
# The priority class needs Main's key (wave.sh exports it); a bare SLOT_PRIORITY=1 from a lane queues as usual.
prio=2; [ "${SLOT_SMALL:-0}" = 1 ] && prio=1
if [ "${SLOT_PRIORITY:-0}" = 1 ]; then
  if [ -n "${SLOT_PRIORITY_KEY:-}" ] && [ "$SLOT_PRIORITY_KEY" = "$(cat "$JET_STATE_ROOT/scratch/Main/.slot-key" 2>/dev/null)" ]; then prio=0
  else echo "build-slot: SLOT_PRIORITY=1 ignored (Main's class only); queueing in class $prio" >&2; fi
fi
# A stage-1 chain build (about 1-2 min, about 6.5 GB) is short: it queues with SLOT_SMALL jobs and may also take slot D,
# which opens only for short builds while MemAvailable > 30 GB, so a quick build never waits behind hour-long suites.
short=0; case " $* " in *"chain.sh --inner "*" s1 "*) short=1;; esac
[ "$short" = 1 ] && [ "$prio" = 2 ] && prio=1
ticket="$Q/$prio-$(date +%s%N)-$$"
: > "$ticket"
trap 'rm -f "$ticket"' EXIT
head_ticket() {
  local t pid
  for t in $(ls "$Q" 2>/dev/null | sort); do
    pid=${t##*-}
    if kill -0 "$pid" 2>/dev/null; then echo "$Q/$t"; return; fi
    rm -f "$Q/$t"
  done
}
# Milliseconds the NVMe disks spent doing I/O, summed.
disk_ms() { awk '$3 ~ /^nvme[0-9]+n[0-9]+$/{t += $13} END{print t+0}' /proc/diskstats; }
busy_prev=$(disk_ms); t_prev=$(date +%s%N); sleep 2
while :; do
  mem=$(awk '/MemAvailable/{print int($2/1048576)}' /proc/meminfo)
  io=$(awk '/^some/{split($2,a,"="); print int(a[2])}' /proc/pressure/io)
  busy_now=$(disk_ms); t_now=$(date +%s%N)
  # Percent of the elapsed wall time the disks (summed) were busy.
  busy=$(( (busy_now - busy_prev) * 100000000 / (t_now - t_prev + 1) ))
  busy_prev=$busy_now; t_prev=$t_now
  [ "$io" -ge 85 ] && [ "$busy" -lt 50 ] && io=0
  # A short build may take slot D out of turn: D serves only short builds, so it never delays the head ticket.
  head=0; [ "$(head_ticket)" = "$ticket" ] && head=1
  if [ "$mem" -gt 20 ] && [ "$io" -lt 85 ] && { [ "$head" = 1 ] || [ "$short" = 1 ]; }; then
    for s in A B C D; do
      [ $s = D ] && { [ "$short" = 1 ] && [ "$mem" -gt 30 ]; } || [ $s != D ] || continue
      [ "$head" = 1 ] || [ $s = D ] || continue
      # Slot C (added 2026-10-23): builds are main-thread bound (load ~20 of 32 with two slots busy), so a third job
      # shares both CPU ranges. It opens only while MemAvailable > 26 GB, so three ~11 GB compiles can't exhaust memory.
      [ $s = C ] && [ "$mem" -le 26 ] && continue
      # A reserved slot (e.g. for a release timing run) is taken only by the reserver (SLOT_RESERVED=1).
      [ -f "$JET_RUN_ROOT/slot-$s-reserved" ] && [ "${SLOT_RESERVED:-0}" != 1 ] && continue
      exec {fd}>"$JET_RUN_ROOT/build-slot-$s.lock"
      if flock -n "$fd"; then
        rm -f "$ticket"
        case $s in A) export SLOT_CPUS=4-9,20-25;; B) export SLOT_CPUS=10-15,26-31;; C) export SLOT_CPUS=4-15,20-31;; D) export SLOT_CPUS=4-15,20-31;; esac
        export CHAIN_CPUS=$SLOT_CPUS CAP_CPUS=$SLOT_CPUS
        # systemd-run service units start from the user manager (core limit unlimited): force LimitCORE=0.
        if [ "$1" = systemd-run ]; then shift; set -- systemd-run -p LimitCORE=0 "$@"; fi
        ulimit -c 0
        # Ledger: one row per job (who, what, how long it waited and ran, peak RSS of its largest process).
        t0=$(date +%s); q0=$(basename "$ticket" | awk -F- '{print int($2/1000000000)}')
        lane=$(printf '%s ' "$PWD" "$@" | awk 'match($0,/(tdev|lanes)\/[A-Za-z0-9]+/){print substr($0,RSTART,RLENGTH)}' | awk -F/ '{print $2}')
        rf=$(mktemp "$JET_RUN_ROOT/slot-rss.XXXXXX")
        # The job runs with the slot lock fd closed ({fd}>&-), so an orphaned descendant can never keep holding the slot.
        # It also runs in its own transient systemd scope with a memory ceiling (2026-10-31: systemd-oomd killed the
        # whole terminal scope, every agent with it, under build memory pressure). Under pressure, the kernel reclaims
        # from and oomd kills the job's scope, never the terminal that launched it. MemorySwapMax (2026-11-01: a
        # release suite pushed its scope 80 GB into swap and took the desktop down) keeps an overrun job from
        # thrashing the machine: it hits its own MemoryMax and dies alone instead.
        memmax=${SLOT_MEMORY_MAX:-16G}
        nice -n 19 ionice -c3 systemd-run --user --scope --quiet --collect -p MemoryHigh="$memmax" -p MemoryMax="$((${memmax%G} + 2))G" -p MemorySwapMax=2G -p ManagedOOMMemoryPressure=kill --unit="jetslot-$s-$$" env time -q -f %M -o "$rf" "$@" {fd}>&-; rc=$?
        printf '%s\t%s\t%s\twait=%ss\trun=%ss\trc=%s\tmaxrss=%sMB\t%s\n' "$(date -d @$t0 +%F_%T)" "$s" "${lane:-?}" "$((t0 - q0))" "$(( $(date +%s) - t0 ))" "$rc" "$(( $(awk 'END{print $1+0}' "$rf") / 1024 ))" "$(printf '%s ' "$@" | cut -c1-200)" >> "$JET_RUN_ROOT/slot-ledger.tsv"
        rm -f "$rf"
        exit $rc
      fi
      exec {fd}>&-
    done
  fi
  sleep 10
done
