# `sh left.sh VERB`: starts `jet VERB spin.jet` as a shell does, its output
# in VERB.out; once a process below jet runs (state R) and has spent half a
# second of CPU (50 clock ticks), past its start and into the compile-time
# work, records each process below jet and says how many; kills jet with
# SIGKILL and says the status it ended with; then waits for the processes
# it recorded to end and says how many are still there after 50 s (a
# zombie has ended), the case's step ending at a minute. Any still there
# are killed, so a failing run leaves nothing spinning.
# A process's children come from its threads' children lists, never a scan
# of every /proc entry, which misses processes that come and go.

# A process's state, then its CPU so far in clock ticks (utime + stime).
state() {
    awk '{ line = $0; sub(/^.*\) /, "", line); split(line, f, " "); print f[1], f[12] + f[13] }' "/proc/$1/stat" 2>/dev/null
}

below() {
    todo=$1
    while [ -n "$todo" ]; do
        next=
        for p in $todo; do
            for c in $(cat /proc/"$p"/task/*/children 2>/dev/null); do
                echo "$c"
                next="$next $c"
            done
        done
        todo=$next
    done
}

running() {
    for p in $(below "$1"); do
        set -- $(state "$p")
        if [ "$1" = R ] && [ "$2" -ge 50 ]; then return 0; fi
    done
    return 1
}

live() {
    n=0
    for p in $pids; do
        set -- $(state "$p")
        if [ -n "$1" ] && [ "$1" != Z ]; then n=$((n + 1)); fi
    done
    echo "$n"
}

jet "$1" spin.jet > "$1.out" 2>&1 &
jet=$!
while [ -n "$(state "$jet")" ] && ! running "$jet"; do
    sleep 0.1
done
pids=$(below "$jet")
echo "below: $(echo $pids | wc -w)"
kill -KILL "$jet"
wait "$jet"
echo "jet: $?"
end=$(($(date +%s) + 50))
left=$(live)
while [ "$left" -gt 0 ] && [ "$(date +%s)" -lt "$end" ]; do
    sleep 0.1
    left=$(live)
done
echo "left: $left"
for p in $pids; do
    kill -KILL "$p" 2>/dev/null
done
