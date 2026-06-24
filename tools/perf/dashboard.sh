#!/usr/bin/env sh
# c121 — compiler performance + compile-time dashboard.
#
# Builds a fixed set of representative programs with JET_TIMING=1, collects the
# per-build jet-timing.json phase reports plus the binary-size line from build
# stderr, aggregates them, and prints a fixed-width table. Optionally diffs
# against a committed baseline.
#
# Usage:
#   tools/perf/dashboard.sh                      # print current timings
#   tools/perf/dashboard.sh --baseline           # write tools/perf/baseline.json
#   tools/perf/dashboard.sh --compare FILE        # diff current vs FILE
#
# std-only: POSIX sh + the jet binary. No external tools, no jq (I6 spirit —
# measurement tooling stays out of Source/ and pulls in nothing exotic).

set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PERF_DIR="$ROOT/tools/perf"
BASELINE="$PERF_DIR/baseline.json"

# Representative programs: tiny, medium, and codegen-heavy.
PROGRAMS="
examples/features/01_hello.jet
examples/features/16_wordcount.jet
examples/features/30_json.jet
examples/features/71_pattern_matching.jet
"

# Run jet via the dev shell so every caller uses the same toolchain.
jet_build() {
    # $1 = source file. Emits jet-timing.json next to the source and a
    # `jet-timing binary_bytes=N` line on stderr.
    JET_TIMING=1 nix develop -c jet build "$1" 2>&1
}

# Pull an integer field "name":N out of a flat jet-timing.json phases array.
# $1 = json file, $2 = phase name. Echoes the µs/byte value or empty.
field() {
    sed 's/},{/}\n{/g' "$1" 2>/dev/null \
        | grep "\"name\":\"$2\"" \
        | sed 's/.*"us"://; s/[^0-9].*//' \
        | head -n1
}

collect() {
    # Echoes lines: "<program> <load> <sema> <ffi> <codegen> <rust_bytes> <binary_bytes>"
    for prog in $PROGRAMS; do
        [ -n "$prog" ] || continue
        src="$ROOT/$prog"
        [ -f "$src" ] || continue
        dir=$(dirname "$src")
        rm -f "$dir/jet-timing.json"
        out=$(cd "$ROOT" && jet_build "$prog")
        bin_bytes=$(printf '%s\n' "$out" | grep 'binary_bytes=' | sed 's/.*binary_bytes=//' | head -n1)
        tj="$dir/jet-timing.json"
        printf '%s %s %s %s %s %s %s\n' \
            "$(basename "$prog")" \
            "$(field "$tj" load)" \
            "$(field "$tj" sema)" \
            "$(field "$tj" ffi)" \
            "$(field "$tj" codegen)" \
            "$(field "$tj" rust_bytes)" \
            "${bin_bytes:-0}"
        rm -f "$tj"
    done
}

print_table() {
    printf '%-26s %8s %8s %8s %8s %10s %10s\n' \
        program load_us sema_us ffi_us codegen_us rust_B binary_B
    echo "$1" | while read -r name load sema ffi codegen rustb binb; do
        [ -n "$name" ] || continue
        printf '%-26s %8s %8s %8s %8s %10s %10s\n' \
            "$name" "$load" "$sema" "$ffi" "$codegen" "$rustb" "$binb"
    done
}

# As-JSON: an array of {program,load,sema,ffi,codegen,rust_bytes,binary_bytes}.
as_json() {
    echo "$1" | awk '
        BEGIN { print "{\"runs\":["; first=1 }
        NF >= 7 {
            if (!first) printf ",\n"; first=0
            printf "  {\"program\":\"%s\",\"load\":%s,\"sema\":%s,\"ffi\":%s,\"codegen\":%s,\"rust_bytes\":%s,\"binary_bytes\":%s}",
                $1, $2, $3, $4, $5, $6, $7
        }
        END { print "\n]}" }'
}

DATA=$(collect)

case "${1:-}" in
    --baseline)
        as_json "$DATA" > "$BASELINE"
        echo "baseline written to $BASELINE"
        print_table "$DATA"
        ;;
    --compare)
        FILE="${2:-$BASELINE}"
        echo "current:"
        print_table "$DATA"
        echo
        echo "baseline: $FILE"
        cat "$FILE" 2>/dev/null || echo "(no baseline file)"
        ;;
    *)
        print_table "$DATA"
        ;;
esac
