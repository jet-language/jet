#!/usr/bin/env bash
# Memory-pressure guard, run as a PreToolUse hook before every Bash command.
# /tmp is RAM-backed tmpfs here: junk left there fills swap and ends in kernel
# OOM kills (2026-08-07: 15G stray cargo target maxed 24G swap overnight).
#
# Fast path (healthy): two file reads, exit 0 — a few ms.
# Pressure path (/tmp >= 70%): auto-delete known-safe stale junk.
# Critical path (still >= 85%, or RAM+swap nearly gone): exit 2, which blocks
# the tool call and shows the message to the agent instead of letting the
# kernel pick a victim.
set -u

uid="$(id -u)"
tmp_use="$(df --output=pcent /tmp 2>/dev/null | tail -1 | tr -dc '0-9')"

mem_critical() {
  local avail_kb swap_total swap_free
  avail_kb="$(awk '/MemAvailable/ {print $2}' /proc/meminfo)"
  swap_total="$(awk '/SwapTotal/ {print $2}' /proc/meminfo)"
  swap_free="$(awk '/SwapFree/ {print $2}' /proc/meminfo)"
  # Critical: under 3G available AND swap over 90% used (or no swap).
  [ "${avail_kb:-999999999}" -lt 3145728 ] || return 1
  [ "${swap_total:-0}" -eq 0 ] && return 0
  [ $((swap_free * 100 / swap_total)) -lt 10 ]
}

clean_stale() {
  # Exited nix develop/shell dirs (liveness-checked by the existing script).
  local here; here="$(dirname "${BASH_SOURCE[0]}")"
  [ -f "$here/clean-nix-tmp.sh" ] && bash "$here/clean-nix-tmp.sh" >/dev/null
  find /tmp -maxdepth 1 -name 'nix-develop-*' -type d -user "$uid" -mmin +60 \
    -exec rm -rf {} + 2>/dev/null
  # Cargo target dirs (CACHEDIR.TAG) untouched for a day — never a live build.
  find /tmp -maxdepth 3 -name CACHEDIR.TAG -user "$uid" 2>/dev/null \
    | while IFS= read -r tag; do
        dir="$(dirname "$tag")"
        find "$dir" -maxdepth 0 -mtime +1 -print -quit 2>/dev/null | grep -q . \
          && rm -rf -- "$dir" 2>/dev/null
      done
  # Large stray files (logs, tars) older than a day.
  find /tmp -maxdepth 1 -type f -user "$uid" -size +200M -mtime +1 \
    -delete 2>/dev/null
  # Agent scratchpad sessions idle for 2+ days.
  find /tmp/claude-1000 -mindepth 2 -maxdepth 2 -type d -mtime +2 \
    -exec rm -rf {} + 2>/dev/null
}

if [ -n "$tmp_use" ] && [ "$tmp_use" -ge 70 ]; then
  clean_stale
  tmp_use="$(df --output=pcent /tmp 2>/dev/null | tail -1 | tr -dc '0-9')"
fi

if { [ -n "$tmp_use" ] && [ "$tmp_use" -ge 85 ]; } || mem_critical; then
  {
    echo "MEMORY PRESSURE CRITICAL — command blocked to prevent a kernel OOM kill."
    echo "/tmp (RAM-backed tmpfs) at ${tmp_use:-?}% after auto-cleanup; $(free -h | awk '/Mem:/ {print $7" RAM available"}; /Swap:/ {print $4" swap free"}' | paste -sd', ')."
    echo "Find and delete the large /tmp items or kill the runaway process before continuing:"
    du -sh /tmp/* 2>/dev/null | sort -rh | head -5
    echo "Never place cargo target dirs or multi-GB logs in /tmp — it is RAM."
  } >&2
  exit 2
fi
exit 0
