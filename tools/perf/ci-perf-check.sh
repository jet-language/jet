#!/usr/bin/env sh
# Compiler-speed CI gate.
#
# Dashboard evidence is valid only when the checked corpus, production stages,
# toolchain, machine, output parity, and variance policy all match. Missing
# evidence fails; it never becomes a zero or a skipped check.

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PERF_DIR="$ROOT/tools/perf"
BASELINE="$PERF_DIR/baseline.json"
THRESH=${1:-}
TAB=$(printf '\t')

[ -s "$BASELINE" ] || { echo "missing baseline: $BASELINE" >&2; exit 1; }

json_string() {
    json_key=$1
    sed 's/,"runs".*//' "$BASELINE" \
        | sed -n 's/.*"'"$json_key"'":"\([^"]*\)".*/\1/p' \
        | head -n1
}

json_number() {
    json_key=$1
    sed 's/,"runs".*//' "$BASELINE" \
        | sed -n 's/.*"'"$json_key"'":\([0-9][0-9]*\).*/\1/p' \
        | head -n1
}

baseline_corpus=$(json_string corpus_sha256)
baseline_version=$(json_number version)
baseline_stage=$(json_string stage)
baseline_os=$(json_string os)
baseline_arch=$(json_string arch)
baseline_target=$(json_string target)
baseline_rustc=$(json_string rustc)
baseline_llvm=$(json_string llvm)
baseline_rustc_vv=$(json_string rustc_vv_sha256)
baseline_compiler=$(json_string compiler_sha256)
baseline_kernel=$(json_string kernel)
baseline_governor=$(json_string governor)
baseline_cpus=$(json_number cpus)
baseline_memory=$(json_number memory_bytes)
baseline_host=$(json_string hostname)
latency_budget=$(json_number latency_regression_pct)
memory_budget=$(json_number memory_regression_pct)
variance_budget=$(json_number variance_pct)
baseline_samples=$(json_number samples)
baseline_warmups=$(json_number warmups)
[ -n "$baseline_version" ] || { echo "baseline has incomplete corpus/stage/machine/budget identity" >&2; exit 1; }
[ "$baseline_version" -eq 3 ] || { echo "unsupported compiler-speed baseline version: $baseline_version" >&2; exit 1; }
for value in "$baseline_corpus" "$baseline_stage" "$baseline_os" "$baseline_arch" "$baseline_target" "$baseline_rustc" "$baseline_llvm" "$baseline_rustc_vv" "$baseline_compiler" "$baseline_kernel" "$baseline_governor" "$baseline_cpus" "$baseline_memory" "$baseline_host" "$latency_budget" "$memory_budget" "$variance_budget" "$baseline_samples" "$baseline_warmups"; do
    [ -n "$value" ] || { echo "baseline has incomplete corpus/stage/machine/budget identity" >&2; exit 1; }
done

case "$THRESH" in
    "") latency_threshold=$latency_budget; memory_threshold=$memory_budget ;;
    *[!0-9]*) echo "threshold must be a non-negative integer" >&2; exit 2 ;;
    *) latency_threshold=$THRESH; memory_threshold=$THRESH ;;
esac

CURRENT=$(TMPDIR=${TMPDIR:-"$HOME/.cache/jet-test-scratch"} "$PERF_DIR/dashboard.sh")
metadata=$(printf '%s\n' "$CURRENT" | sed -n '1p')
current_version=$(printf '%s\n' "$metadata" | sed -n 's/.*version=\([^ ]*\).*/\1/p')
current_corpus=$(printf '%s\n' "$metadata" | sed -n 's/.*corpus_sha256=\([^ ]*\).*/\1/p')
current_stage=$(printf '%s\n' "$metadata" | sed -n 's/.*stage=\([^ ]*\).*/\1/p')
current_machine=$(printf '%s\n' "$metadata" | sed -n 's/.*machine=\([^ ]*\).*/\1/p')
current_target=$(printf '%s\n' "$metadata" | sed -n 's/.*target=\([^ ]*\).*/\1/p')
current_rustc=$(printf '%s\n' "$metadata" | sed -n 's/.*rustc=\([^ ]*\).*/\1/p')
current_llvm=$(printf '%s\n' "$metadata" | sed -n 's/.*llvm=\([^ ]*\).*/\1/p')
current_rustc_vv=$(printf '%s\n' "$metadata" | sed -n 's/.*rustc_vv_sha256=\([^ ]*\).*/\1/p')
current_compiler=$(printf '%s\n' "$metadata" | sed -n 's/.*compiler_sha256=\([^ ]*\).*/\1/p')
current_kernel=$(printf '%s\n' "$metadata" | sed -n 's/.*kernel=\([^ ]*\).*/\1/p')
current_governor=$(printf '%s\n' "$metadata" | sed -n 's/.*governor=\([^ ]*\).*/\1/p')
current_memory=$(printf '%s\n' "$metadata" | sed -n 's/.*memory_bytes=\([^ ]*\).*/\1/p')
current_samples=$(printf '%s\n' "$metadata" | sed -n 's/.*samples=\([^ ]*\).*/\1/p')
current_warmups=$(printf '%s\n' "$metadata" | sed -n 's/.*warmups=\([^ ]*\).*/\1/p')
current_os=$(printf '%s\n' "$current_machine" | cut -d/ -f1)
current_arch=$(printf '%s\n' "$current_machine" | cut -d/ -f2)
current_cpus=$(printf '%s\n' "$current_machine" | sed 's/.*cpus=\([^/]*\).*/\1/')
current_host=$(printf '%s\n' "$current_machine" | sed 's/.*host=//')

check_identity() {
    identity_name=$1
    identity_current=$2
    identity_baseline=$3
    [ "$identity_current" = "$identity_baseline" ] || {
        echo "$identity_name changed: $identity_baseline -> $identity_current" >&2
        exit 1
    }
}

check_identity corpus "$current_corpus" "$baseline_corpus"
check_identity report-version "$current_version" "$baseline_version"
check_identity stage "$current_stage" "$baseline_stage"
check_identity OS "$current_os" "$baseline_os"
check_identity architecture "$current_arch" "$baseline_arch"
check_identity target "$current_target" "$baseline_target"
check_identity rustc "$current_rustc" "$baseline_rustc"
check_identity LLVM "$current_llvm" "$baseline_llvm"
check_identity rustc-vV "$current_rustc_vv" "$baseline_rustc_vv"
check_identity compiler "$current_compiler" "$baseline_compiler"
check_identity kernel "$current_kernel" "$baseline_kernel"
check_identity governor "$current_governor" "$baseline_governor"
check_identity CPU-count "$current_cpus" "$baseline_cpus"
check_identity machine-memory "$current_memory" "$baseline_memory"
check_identity host "$current_host" "$baseline_host"
check_identity samples "$current_samples" "$baseline_samples"
check_identity warmups "$current_warmups" "$baseline_warmups"

baseline_field() {
    field_program=$1
    field_state=$2
    field_name=$3
    sed 's/},{/}\n{/g' "$BASELINE" \
        | grep -F '"program":"'"$field_program"'","state":"'"$field_state"'"' \
        | sed 's/.*"'"$field_name"'"://; s/[",}].*//' \
        | head -n1
}

FAIL=0
ROW_COUNT=0
CURRENT_ROWS="$TMPDIR/jet-compiler-speed-ci-$$.rows"
trap 'rm -f "$CURRENT_ROWS"' EXIT HUP INT TERM
printf '%s\n' "$CURRENT" | tail -n +3 > "$CURRENT_ROWS"

# Rows have no spaces before the phase field, so a tab read remains
# unambiguous.
while read -r row_program row_state row_stage row_latency row_memory row_variance row_output row_phases; do
    [ -n "${row_program:-}" ] || continue
    ROW_COUNT=$((ROW_COUNT + 1))
    case "$row_latency:$row_memory:$row_variance" in
        ''|*[!0-9:]*|*::*|*:*:) echo "incomplete current timing row: $row_program/$row_state" >&2; exit 1 ;;
    esac
    base_stage=$(baseline_field "$row_program" "$row_state" stage)
    base_latency=$(baseline_field "$row_program" "$row_state" latency_ns)
    base_memory=$(baseline_field "$row_program" "$row_state" memory_bytes)
    base_variance=$(baseline_field "$row_program" "$row_state" variance_pct)
    base_stdout=$(baseline_field "$row_program" "$row_state" stdout_sha256)
    base_stderr=$(baseline_field "$row_program" "$row_state" stderr_sha256)
    for value in "$base_stage" "$base_latency" "$base_memory" "$base_variance" "$base_stdout" "$base_stderr"; do
        [ -n "$value" ] || { echo "baseline missing row: $row_program/$row_state" >&2; exit 1; }
    done
    [ "$row_stage" = "$base_stage" ] || { echo "stage changed for $row_program/$row_state" >&2; exit 1; }
    case "$base_latency:$base_memory:$base_variance" in
        *[!0-9:]*|*::*|*:*:) echo "invalid baseline row: $row_program/$row_state" >&2; exit 1 ;;
    esac
    for metric in "latency_ns:$row_latency:$base_latency:$latency_threshold" "memory_bytes:$row_memory:$base_memory:$memory_threshold"; do
        metric_name=${metric%%:*}
        metric_rest=${metric#*:}
        metric_current=${metric_rest%%:*}
        metric_rest=${metric_rest#*:}
        metric_base=${metric_rest%%:*}
        metric_limit=${metric_rest#*:}
        delta=$(( (metric_current - metric_base) * 100 / metric_base ))
        if [ "$delta" -gt "$metric_limit" ]; then
            echo "REGRESSION $row_program/$row_state $metric_name: $metric_base -> $metric_current (+${delta}%, threshold ${metric_limit}%)" >&2
            FAIL=1
        fi
    done
    if [ "$row_variance" -gt "$variance_budget" ]; then
        echo "UNSTABLE $row_program/$row_state variance=${row_variance}% budget=${variance_budget}%" >&2
        FAIL=1
    fi
    row_stdout=${row_output%%:*}
    row_stderr=${row_output#*:}
    [ "$row_stdout" = "$base_stdout" ] || { echo "stdout parity changed for $row_program/$row_state" >&2; FAIL=1; }
    [ "$row_stderr" = "$base_stderr" ] || { echo "stderr parity changed for $row_program/$row_state" >&2; FAIL=1; }
    case "$row_phases" in
        *phases=*source=*cache_hits=*cache_misses=*top_cause=*backend=*linker=*artifact_bytes=*) ;;
        *) echo "missing phase totals for $row_program/$row_state" >&2; FAIL=1 ;;
    esac
    case "$row_phases" in
        *linker=unavailable*) echo "missing linker identity for $row_program/$row_state" >&2; FAIL=1 ;;
    esac
done < "$CURRENT_ROWS"

expected_rows=$((4 * 6))
[ "$ROW_COUNT" -eq "$expected_rows" ] || {
    echo "checked corpus row count changed: expected $expected_rows, got $ROW_COUNT" >&2
    exit 1
}
[ "$FAIL" -eq 0 ] || { echo "perf gate FAILED" >&2; exit 1; }
echo "perf gate OK (latency ${latency_threshold}%, memory ${memory_threshold}%, variance ${variance_budget}%, rows ${ROW_COUNT})"
