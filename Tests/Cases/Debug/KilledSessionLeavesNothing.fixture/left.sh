# The processes below a `jet debug` session, and whether any outlives it.
# `sh left.sh record PID`: each process below PID to left.pids, one a line,
# and how many there are and the state of the last (the program).
# `sh left.sh count`: waits up to 10 s for those to end, then says how many
# are still there; a zombie has ended.
state() {
    awk '{ line = $0; sub(/^.*\) /, "", line); split(line, f, " "); print f[1] }' "/proc/$1/stat" 2>/dev/null
}

children() {
    awk -v want="$1" '{ line = $0; sub(/^.*\) /, "", line); split(line, f, " "); if (f[2] == want) { n = FILENAME; sub(/^\/proc\//, "", n); sub(/\/stat$/, "", n); print n } }' /proc/[0-9]*/stat 2>/dev/null
}

case "$1" in
record)
    : > left.pids
    todo=$2
    last=
    while [ -n "$todo" ]; do
        next=
        for p in $todo; do
            for c in $(children "$p"); do
                echo "$c" >> left.pids
                next="$next $c"
                last=$c
            done
        done
        todo=$next
    done
    echo "below: $(wc -l < left.pids)"
    echo "program: $(state "$last")"
    ;;
count)
    tries=0
    while :; do
        live=0
        for p in $(cat left.pids); do
            s=$(state "$p")
            if [ -n "$s" ] && [ "$s" != Z ]; then live=$((live + 1)); fi
        done
        if [ "$live" -eq 0 ] || [ "$tries" -ge 100 ]; then break; fi
        sleep 0.1
        tries=$((tries + 1))
    done
    echo "left: $live"
    ;;
esac
