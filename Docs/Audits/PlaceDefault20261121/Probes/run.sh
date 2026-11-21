#!/usr/bin/env bash
# Runs the placement review's probes one at a time and prints each command
# with its output. `jet` on PATH is the compiler under test; `node` runs the
# web build. Usage: Probes/run.sh > run.log
# The log is committed, so every line names the machine's paths by their
# placeholders (neutral): the work directory, the scratch and state roots
# and the home directory.
set -u
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
pin="taskset -c 16-31"

step() {
    printf '\n$ %s\n' "$*"
    "$@" 2>&1
    printf '(exit %s)\n' "$?"
}

# Stdin with each machine path replaced by its placeholder, the longest
# first: the state root lies under the home directory.
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

# Builds probe `name` at each listed level into $work/name/O<n>/main.
build() {
    local name=$1
    shift
    mkdir -p "$work/$name"
    cp "$here/$name.jet" "$work/$name/main.jet"
    for level in "$@"; do
        (cd "$work/$name" && step nice -n 19 jet build "-$level" "--output-dir=$level" main.jet)
    done
}

# The allocation rows of one `jet perf run --lens=alloc` capture.
alloc_lens() {
    local name=$1
    shift
    (cd "$work/$name" && step nice -n 19 jet perf run -O2 --lens=alloc main.jet --out lens.jettrace -- "$@")
    (cd "$work/$name" && jet perf view lens.jettrace 2>&1 | awk '/^allocated|main\.jet:[0-9]+ [a-z_]+ +[0-9]+ allocations/')
}

{
printf 'jet %s\n' "$(command -v jet)"
printf 'identity %s\n' "${JET_COMPILER_IDENTITY:-unset}"
printf 'node %s\n' "$(node --version)"
uptime

build alloc O0 O1 O2
for level in O0 O1 O2; do
    for kernel in words ints; do
        for mode in heap scratch arena; do
            step $pin "$work/alloc/$level/main" "$kernel" "$mode" 2000
        done
    done
    for kernel in count reserved small reuse; do
        for mode in heap scratch; do
            step $pin "$work/alloc/$level/main" "$kernel" "$mode" 2000
        done
    done
done
for kernel in words count ints small; do
    alloc_lens alloc "$kernel" heap 10
done

mkdir -p "$work/pushes"
cp "$here/pushes.jet" "$work/pushes/main.jet"
alloc_lens pushes

build peak O0 O2
for level in O0 O2; do
    for mode in heap round scope; do
        step $pin "$work/peak/$level/main" "$mode" 200
    done
done

build tasks O2
for mode in none heap scratch; do
    for count in 1000 10000; do
        step $pin timeout 120 "$work/tasks/O2/main" "$mode" "$count"
    done
done

build sends O2
for size in 16 1000; do
    for mode in heap scratch; do
        step $pin "$work/sends/O2/main" "$mode" "$size" 20000
    done
done
for mode in heap scratch; do
    step $pin "$work/sends/O2/main" "$mode" 100000 500
done

build recur O0 O2
for level in O0 O2; do
    for mode in heap scratch; do
        step $pin "$work/recur/$level/main" "$mode" 14 20
    done
done
alloc_lens recur heap 14 1

build web O2
for _ in 1 2 3; do
    step $pin "$work/web/O2/main"
done
(cd "$work/web" && step nice -n 19 jet build --os=web main.jet)
for _ in 1 2 3; do
    (cd "$work/web/.jet/build" && step $pin node app.js)
done

uptime
} 2>&1 | neutral
rm -rf "$work"
