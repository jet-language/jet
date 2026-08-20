#!/usr/bin/env bash
# lane-guardian.sh — protect a many-lane session from the two ways it can end badly:
# losing work, and dying of OOM.
#
# Every INTERVAL seconds it does two things.
#
#   1. Snapshots the working tree to a timestamped patch plus a tarball of
#      untracked files, under ~/.cache/jet-luna/snapshots. Many agents share ONE
#      working tree here, so two of them can write the same file and the second
#      write wins. A snapshot every few minutes bounds what a clobber can cost,
#      and unlike a commit it cannot itself break anything.
#
#   2. Watches memory. This machine has died of OOM in this exact situation
#      before: /tmp is RAM-backed tmpfs and concurrent rustc invocations reached
#      52G of 61G. If available memory falls under the floor, it stops the
#      newest worker processes until it is back over the floor, newest first,
#      because the newest lane has done the least work worth keeping.
#
# Usage: scripts/agent/lane-guardian.sh [interval_seconds] [floor_gb]

set -u

INTERVAL="${1:-180}"
FLOOR_GB="${2:-8}"
OUT="$HOME/.cache/jet-luna/snapshots"
LOG="$HOME/.cache/jet-luna/guardian.log"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

mkdir -p "$OUT"

note() { printf '%s %s\n' "$(date +%H:%M:%S)" "$*" >>"$LOG"; }

note "guardian start: interval=${INTERVAL}s floor=${FLOOR_GB}G repo=$REPO"

while true; do
    stamp="$(date +%H%M%S)"

    # --- snapshot -------------------------------------------------------
    if git -C "$REPO" diff HEAD >"$OUT/wip-$stamp.patch" 2>/dev/null; then
        bytes=$(wc -c <"$OUT/wip-$stamp.patch")
        if [ "$bytes" -lt 40 ]; then
            rm -f "$OUT/wip-$stamp.patch"
        else
            git -C "$REPO" status --porcelain 2>/dev/null \
                | sed -n 's/^?? //p' >"$OUT/untracked-$stamp.txt"
            if [ -s "$OUT/untracked-$stamp.txt" ]; then
                tar czf "$OUT/untracked-$stamp.tgz" -C "$REPO" \
                    -T "$OUT/untracked-$stamp.txt" 2>/dev/null
            fi
            rm -f "$OUT/untracked-$stamp.txt"
            note "snapshot wip-$stamp.patch ($bytes bytes)"
        fi
    fi

    # Keep the last 40 snapshots; they are small and disk here is not the
    # constraint, but unbounded growth is how the last cleanup got to 630G.
    ls -1t "$OUT"/wip-*.patch 2>/dev/null | tail -n +41 | while read -r old; do
        rm -f "$old"
    done
    ls -1t "$OUT"/untracked-*.tgz 2>/dev/null | tail -n +41 | while read -r old; do
        rm -f "$old"
    done

    # --- memory ---------------------------------------------------------
    avail=$(free -g | awk '/^Mem:/ {print $7}')
    if [ "${avail:-99}" -lt "$FLOOR_GB" ]; then
        note "MEMORY LOW: ${avail}G available, floor ${FLOOR_GB}G"
        # Newest codex worker first: it has produced the least so far.
        for pid in $(pgrep -f 'codex exec' | tac); do
            avail=$(free -g | awk '/^Mem:/ {print $7}')
            [ "${avail:-99}" -ge "$FLOOR_GB" ] && break
            note "stopping worker pid $pid to reclaim memory"
            kill -TERM "$pid" 2>/dev/null
            sleep 5
        done
        note "memory after reclaim: $(free -g | awk '/^Mem:/ {print $7}')G"
    fi

    sleep "$INTERVAL"
done
