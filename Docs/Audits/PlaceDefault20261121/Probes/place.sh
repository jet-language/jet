#!/usr/bin/env bash
# Runs the probes of the revised ballot on values returned onto a caller's
# stack, and prints each command with its output: place.jet (one builder
# body against a builder with a stack arm), convention.mjs (a web call's
# destination as a parameter or a wasm global), register.py (an x64 call's
# destination as an argument or in RAX) and the debugger's view of a hidden
# parameter. `jet` on PATH is the compiler under test; `node` runs the web
# build and convention.mjs. Usage: Probes/place.sh > place.log
# The log is committed, so every line names the machine's paths by their
# placeholders, as returns.sh does.
set -u
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
pin=${PLACE_PIN:-taskset -c 16-31}

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
        line=${line//"$here"/Probes}
        line=${line//"$work"/\$work}
        line=${line//"$scratch"/\$JET_SCRATCH_ROOT}
        line=${line//"$state"/\$JET_STATE_ROOT}
        line=${line//"$HOME"/\~}
        printf '%s\n' "$line"
    done
}

mkdir -p "$work/place"
cp "$here/place.jet" "$work/place/main.jet"
cd "$work/place" || exit 1
{
    # The checkout the probe ran from, and the identity of the compiler under
    # test: the build's IDENTITY beside `jet`, or JET_COMPILER_IDENTITY.
    step git -C "$here" rev-parse --short HEAD
    printf '\n$ jet identity\n%s\n' "${JET_COMPILER_IDENTITY:-$(cat "$(dirname "$(command -v jet)")/IDENTITY" 2>&1)}"
    step jet --version
    step node --version
    step python3 --version
    uptime
    for level in O0 O1 O2; do
        step nice -n 19 jet build "-$level" "--output-dir=$level" main.jet
    done
    step nice -n 19 jet build --os=web main.jet
    # Each builder's code size, from the O2 image's symbol table: the stack
    # arm's size is build_arm's less build_to's.
    printf '\n$ jet debug O2/main --do symbols   (the builders)\n'
    jet debug O2/main --do symbols 2>&1 | awk '$4 ~ /^main\.build/'
    # A destination passed as a Jet parameter is a local the debugger lists:
    # build_to's `dest` beside build's locals. The compiler's own hidden
    # word would be marked artificial, which Locals skips.
    step jet debug main.jet --break main.jet:44 --do continue --do locals -- arg direct 2 1 0
    step jet debug main.jet --break main.jet:37 --do continue --do locals -- plain direct 2 1 0
    # The conventions alternate within each round, so a busy host's drift
    # lands on all of them alike.
    for level in O0 O1 O2; do
        for form in direct value closure; do
            for _ in 1 2 3 4 5; do
                for conv in plain arg arm local place arena; do
                    step $pin "$level/main" "$conv" "$form" 2 2000 0
                done
            done
        done
    done
    (
        cd .jet/build || exit 1
        for form in direct value; do
            for _ in 1 2 3 4 5; do
                for conv in plain arg arm local place arena; do
                    step $pin node app.js "$conv" "$form" 2 500 0
                done
            done
        done
    )
    step $pin node "$here/convention.mjs" 20000000 9
    step $pin python3 "$here/register.py" 200000000 9
    uptime
} 2>&1 | neutral
cd / && rm -rf "$work"
