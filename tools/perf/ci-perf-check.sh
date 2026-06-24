#!/usr/bin/env sh
# c121 — CI compile-time regression check.
#
# Builds the representative programs with timing on, compares the sema phase
# (the dominant, most-stable compile cost) and binary size against the
# committed baseline. Exits nonzero if any metric regresses by more than the
# threshold, so CI fails loudly on an accidental slowdown.
#
# The owner updates the baseline deliberately after an intentional change with
#   tools/perf/update-baseline.sh
#
# Usage: tools/perf/ci-perf-check.sh [threshold_percent]   (default 15)

set -eu

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PERF_DIR="$ROOT/tools/perf"
BASELINE="$PERF_DIR/baseline.json"
THRESH="${1:-15}"

if [ ! -f "$BASELINE" ]; then
    echo "no baseline at $BASELINE — run tools/perf/update-baseline.sh first"
    echo "(skipping perf gate)"
    exit 0
fi

# Read baseline metric for a program: $1 = program basename, $2 = field name.
baseline_field() {
    sed 's/},/}\n/g' "$BASELINE" \
        | grep "\"program\":\"$1\"" \
        | sed "s/.*\"$2\"://; s/[^0-9].*//" \
        | head -n1
}

CURRENT=$("$PERF_DIR/dashboard.sh")
FAIL=0

echo "$CURRENT" | tail -n +2 | while read -r name load sema ffi codegen rustb binb; do
    [ -n "$name" ] || continue
    for metric in sema:$sema binary_bytes:$binb; do
        field=$(echo "$metric" | cut -d: -f1)
        cur=$(echo "$metric" | cut -d: -f2)
        base=$(baseline_field "$name" "$field")
        [ -n "$base" ] && [ "$base" -gt 0 ] 2>/dev/null || continue
        # delta% = (cur - base) * 100 / base
        delta=$(( (cur - base) * 100 / base ))
        if [ "$delta" -gt "$THRESH" ]; then
            echo "REGRESSION  $name $field: $base -> $cur (+${delta}%, threshold ${THRESH}%)"
            FAIL=1
        fi
    done
    # `while` runs in a subshell; signal failure via a marker file.
    [ "$FAIL" -eq 0 ] || echo fail > "$PERF_DIR/.ci-fail"
done

if [ -f "$PERF_DIR/.ci-fail" ]; then
    rm -f "$PERF_DIR/.ci-fail"
    echo "perf gate FAILED"
    exit 1
fi
echo "perf gate OK (threshold ${THRESH}%)"
