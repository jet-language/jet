#!/usr/bin/env sh
# Compiler-speed CI gate.
#
# It requires the checked corpus, optimized AOT stage, and pinned machine
# identity to match the committed baseline. Missing or changed evidence fails;
# it never turns an unavailable measurement into a pass.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PERF_DIR="$ROOT/tools/perf"
BASELINE="$PERF_DIR/baseline.json"
THRESH=${1:-15}

case "$THRESH" in
    ''|*[!0-9]*) echo "threshold must be a non-negative integer" >&2; exit 2 ;;
esac
[ -s "$BASELINE" ] || { echo "missing baseline: $BASELINE" >&2; exit 1; }

json_field() {
    key=$1
    sed -n 's/.*"'$key'":"\([^"]*\)".*/\1/p' "$BASELINE" | head -n1
}

baseline_corpus=$(json_field corpus_sha256)
baseline_stage=$(json_field stage)
baseline_os=$(json_field os)
baseline_arch=$(json_field arch)
baseline_cpus=$(sed -n 's/.*"cpus":\([0-9][0-9]*\).*/\1/p' "$BASELINE" | head -n1)
baseline_host=$(json_field hostname)
for value in "$baseline_corpus" "$baseline_stage" "$baseline_os" "$baseline_arch" "$baseline_cpus" "$baseline_host"; do
    [ -n "$value" ] || { echo "baseline has no pinned corpus/stage/machine identity" >&2; exit 1; }
done

CURRENT=$("$PERF_DIR/dashboard.sh")
metadata=$(printf '%s\n' "$CURRENT" | sed -n '1p')
current_corpus=$(printf '%s\n' "$metadata" | sed -n 's/.*corpus_sha256=\([^ ]*\).*/\1/p')
current_stage=$(printf '%s\n' "$metadata" | sed -n 's/.*stage=\([^ ]*\).*/\1/p')
current_machine=$(printf '%s\n' "$metadata" | sed -n 's/.*machine=\([^ ]*\).*/\1/p')
current_os=$(printf '%s\n' "$current_machine" | cut -d/ -f1)
current_arch=$(printf '%s\n' "$current_machine" | cut -d/ -f2)
current_cpus=$(printf '%s\n' "$current_machine" | sed 's/.*cpus=\([^/]*\).*/\1/')
current_host=$(printf '%s\n' "$current_machine" | sed 's/.*host=//')

[ "$current_corpus" = "$baseline_corpus" ] || { echo "corpus changed: $baseline_corpus -> $current_corpus" >&2; exit 1; }
[ "$current_stage" = "$baseline_stage" ] || { echo "stage changed: $baseline_stage -> $current_stage" >&2; exit 1; }
[ "$current_os" = "$baseline_os" ] || { echo "OS changed: $baseline_os -> $current_os" >&2; exit 1; }
[ "$current_arch" = "$baseline_arch" ] || { echo "architecture changed: $baseline_arch -> $current_arch" >&2; exit 1; }
[ "$current_cpus" = "$baseline_cpus" ] || { echo "CPU count changed: $baseline_cpus -> $current_cpus" >&2; exit 1; }
[ "$current_host" = "$baseline_host" ] || { echo "host changed: $baseline_host -> $current_host" >&2; exit 1; }

baseline_field() {
    program=$1
    field=$2
    sed 's/},/}\n/g' "$BASELINE" \
        | grep -F '"program":"'$program'"' \
        | sed 's/.*"'$field'"://; s/[^0-9].*//' \
        | head -n1
}

FAIL=0
# First line is identity; second is table header.
while read -r program stage load sema ffi codegen backend rust_bytes binary_bytes; do
    [ -n "$program" ] || continue
    case "$sema:$binary_bytes" in
        *[!0-9:]*|*:|:*) echo "incomplete current timing row: $program" >&2; exit 1 ;;
    esac
    for metric in sema:$sema binary_bytes:$binary_bytes; do
        field=${metric%%:*}
        current=${metric#*:}
        base=$(baseline_field "$program" "$field")
        case "$base" in
            ''|0|*[!0-9]*) echo "baseline missing or zero $program/$field" >&2; exit 1 ;;
        esac
        delta=$(( (current - base) * 100 / base ))
        if [ "$delta" -gt "$THRESH" ]; then
            echo "REGRESSION $program $field: $base -> $current (+${delta}%, threshold $THRESH%)" >&2
            FAIL=1
        fi
    done
done <<EOF
$(printf '%s\n' "$CURRENT" | tail -n +3)
EOF

[ "$FAIL" -eq 0 ] || { echo "perf gate FAILED" >&2; exit 1; }
echo "perf gate OK (threshold $THRESH%)"
