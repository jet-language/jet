#!/usr/bin/env bash
# Runs returns.jet, the calling-convention probe for the ballot on values
# returned onto a caller's stack, and prints each command with its output.
# `jet` on PATH is the compiler under test; `node` runs the web build. Each
# row runs five times. Usage: Probes/returns.sh > returns.log
# The log is committed, so every line names the machine's paths by their
# placeholders, as run.sh does.
set -u
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
pin="taskset -c 16-31"

# Each step prints its exit status and the host's one-minute load average as
# it ends, so a reader can see what each row ran beside.
step() {
    printf '\n$ %s\n' "$*"
    "$@" 2>&1
    local status=$?
    printf '(exit %s, load %s)\n' "$status" "$(cut -d' ' -f1 /proc/loadavg)"
}

neutral() {
    local state=${JET_STATE_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/jet-dev}
    local scratch=${JET_SCRATCH_ROOT:-$state}
    local line
    while IFS= read -r line || [[ -n $line ]]; do
        line=${line//"$work"/\$work}
        line=${line//"$scratch"/\$JET_SCRATCH_ROOT}
        line=${line//"$state"/\$JET_STATE_ROOT}
        line=${line//"$HOME"/\~}
        printf '%s\n' "$line"
    done
}

mkdir -p "$work/returns"
cp "$here/returns.jet" "$work/returns/main.jet"
cd "$work/returns" || exit 1
{
    step jet --version
    step node --version
    uptime
    for level in O0 O1 O2; do
        step nice -n 19 jet build "-$level" "--output-dir=$level" main.jet
    done
    step nice -n 19 jet build --os=web main.jet
    # Each builder's code size, from the O2 image's symbol table.
    printf '\n$ jet debug O2/main --do symbols   (the builders)\n'
    jet debug O2/main --do symbols 2>&1 | awk '$4 ~ /^main\.(build|make|copy_)/'
    # The conventions alternate within each round, so a busy host's drift
    # lands on all of them alike.
    for level in O0 O1 O2; do
        for form in direct value trait closure; do
            for _ in 1 2 3 4 5; do
                for conv in plain arg branch test slot copy sized; do
                    step $pin "$level/main" "$conv" "$form" 2 2000 0
                done
            done
        done
    done
    # The copy a caller makes when its own rule places the result on the
    # heap grows with the value; the call itself does not.
    for items in 16 1000; do
        for _ in 1 2 3 4 5; do
            for conv in plain copy sized; do
                step $pin O2/main "$conv" direct "$items" $((40000 / items)) 0
            done
        done
    done
    cd .jet/build || exit 1
    for form in direct value; do
        for _ in 1 2 3 4 5; do
            for conv in plain arg test slot; do
                step $pin node app.js "$conv" "$form" 2 500 0
            done
        done
    done
    uptime
} 2>&1 | neutral
cd / && rm -rf "$work"
