# The processes below a `jet debug` session, and whether any outlives it.
# `sh left.sh record PID STATE`: each process below PID to left.pids, one a
# line, once the last of them (the program) is in STATE; then how many
# there are and the program's state.
# `sh left.sh count`: waits for those to end, then says how many are still
# there; a zombie has ended. A process still there after 50 s counts (the
# case's step ends at a minute).
# A process's children come from its threads' children lists, never a scan
# of every /proc entry: awk ends at the first entry whose process exits
# while it reads, so on a busy machine a scan misses processes.
state() {
    if [ -z "$1" ]; then
        echo none
        return
    fi
    awk '{ line = $0; sub(/^.*\) /, "", line); split(line, f, " "); print f[1] }' "/proc/$1/stat" 2>/dev/null
}

children() {
    cat /proc/"$1"/task/*/children 2>/dev/null
}

live() {
    n=0
    for p in $(cat left.pids); do
        s=$(state "$p")
        if [ -n "$s" ] && [ "$s" != Z ]; then n=$((n + 1)); fi
    done
    echo "$n"
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
    while [ -n "$last" ] && [ -n "$(state "$last")" ] && [ "$(state "$last")" != "$3" ]; do
        sleep 0.1
    done
    echo "below: $(wc -l < left.pids)"
    echo "program: $(state "$last")"
    ;;
count)
    end=$(($(date +%s) + 50))
    left=$(live)
    while [ "$left" -gt 0 ] && [ "$(date +%s)" -lt "$end" ]; do
        sleep 0.1
        left=$(live)
    done
    echo "left: $left"
    ;;
esac
