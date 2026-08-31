#!/usr/bin/env bash
# lane-keeper.sh — recycle one explicitly authorized direct-CLI fallback lane.
#
# OMP task/hub owns normal dispatch. This loop stays dormant unless the caller
# records the exact OMP failure in JET_OMP_FALLBACK_REASON; it cannot silently
# become a second dispatch route.
#
# It never closes a card and never touches the board beyond reading it. Judging
# a lane's output and closing its card stays with the orchestrator, because that
# is the part that needs taste.
#
# Usage: scripts/agent/lane-keeper.sh [interval_seconds] [min_free_gb]

set -u

INTERVAL="${1:-180}"
MIN_FREE_GB="${2:-14}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LOG="$HOME/.cache/jet-luna/keeper.log"
DISPATCH="$REPO/scripts/agent/lane-dispatch.mjs"

if [ -z "${JET_OMP_FALLBACK_REASON:-}" ]; then
    printf '%s\n' "lane-keeper disabled: use OMP task/hub; set JET_OMP_FALLBACK_REASON only after recording the exact failure" >&2
    exit 2
fi

note() { printf '%s %s\n' "$(date +%H:%M:%S)" "$*" >>"$LOG"; }

note "keeper start: interval=${INTERVAL}s floor=${MIN_FREE_GB}G"

while true; do
    free_gb=$(free -g | awk '/^Mem:/ {print $7}')
    if [ "${free_gb:-0}" -lt "$MIN_FREE_GB" ]; then
        note "holding: ${free_gb}G free, floor ${MIN_FREE_GB}G"
        sleep "$INTERVAL"
        continue
    fi

    status=$(cd "$REPO" && node "$DISPATCH" status 2>/dev/null)
    room=$(printf '%s' "$status" | sed -n 's/.*ROOM FOR \([0-9]*\) MORE.*/\1/p')

    if [ -z "${room:-}" ] || [ "$room" -le 0 ]; then
        sleep "$INTERVAL"
        continue
    fi

    # Prefer cards that have never had a lane. `brief --auto` already skips any
    # card with an existing log and is stale-blocker aware, so it will not hand
    # back something already in flight or genuinely gated.
    fresh=$(cd "$REPO" && node "$DISPATCH" brief --auto "$room" 2>/dev/null | tr -d '\n')

    # When every open card has already had a lane, recycle the least-recently
    # worked unfinished card. The dispatcher defaults to one stream.
    if [ -z "$fresh" ] || [ "$fresh" = "(nothing to brief)" ]; then
        fresh=$(cd "$REPO" && node "$DISPATCH" recycle "$room" 2>/dev/null | tr -d '\n')
        [ -n "${fresh// /}" ] && note "recycling open cards: $fresh"
    fi

    if [ -n "${fresh// /}" ]; then
        # shellcheck disable=SC2086
        out=$(cd "$REPO" && node "$DISPATCH" launch $fresh 2>&1 | tail -n 1)
        note "$out"
    else
        note "room for $room but nothing to launch"
    fi

    sleep "$INTERVAL"
done
