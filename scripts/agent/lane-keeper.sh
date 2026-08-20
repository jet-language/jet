#!/usr/bin/env bash
# lane-keeper.sh — keep every lane slot full, without a human in the loop.
#
# The orchestrator's attention is the scarce resource in a burndown, and it was
# going almost entirely to noticing that lanes had finished and starting more.
# Measured repeatedly in one session: the running count fell from 25 to 1 while
# the orchestrator was busy repairing a build, and the whole wave idled.
#
# This closes that gap. Every INTERVAL seconds it asks lane-dispatch how many
# lanes are actually alive — by pidfile, not by log — and if there is room it
# briefs more open cards straight from Tower and launches them detached.
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

    # When every open card has already had a lane, restart the ones that
    # produced no report. A lane that timed out still leaves its partial work in
    # the tree, so a second pass usually gets further than the first.
    if [ -z "$fresh" ] || [ "$fresh" = "(nothing to brief)" ]; then
        fresh=$(printf '%s' "$status" \
            | sed -n 's/.*no report (re-brief smaller): //p' \
            | tr ' ' '\n' \
            | grep -E '^(c[0-9]+|[a-z][a-z0-9]+)$' \
            | grep -vE '^(corpus-shard|test-shard|dotarrow|persist)' \
            | head -n "$room" \
            | tr '\n' ' ')
        [ -n "$fresh" ] && note "no unstarted cards; restarting stalled: $fresh"
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
