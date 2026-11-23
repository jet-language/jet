#!/bin/sh
# usage: sh comptime.sh <pid> [signal]
# Waits, up to 30 seconds, for the `jet-comptime` process that `jet build`
# (<pid>, or a process below it) runs `build: fn` in, then half a second more
# so `build: fn` is midway. With a signal, sends that process the signal, as
# the system's SIGKILL when memory runs out. Prints `running` once it ran.
root=$1
signal=${2:-}

find_comptime() {
    queue=$1
    while [ -n "$queue" ]; do
        next=""
        for pid in $queue; do
            if [ "$(cat "/proc/$pid/comm" 2>/dev/null)" = "jet-comptime" ]; then
                echo "$pid"
                return 0
            fi
            next="$next $(pgrep -P "$pid")"
        done
        queue=$(echo $next)
    done
    return 1
}

tries=0
while [ "$tries" -lt 600 ]; do
    comptime=$(find_comptime "$root")
    if [ -n "$comptime" ]; then
        sleep 0.5
        if [ -n "$signal" ]; then
            kill -s "$signal" "$comptime"
        fi
        echo "running"
        exit 0
    fi
    tries=$((tries + 1))
    sleep 0.05
done
echo "never ran"
exit 1
