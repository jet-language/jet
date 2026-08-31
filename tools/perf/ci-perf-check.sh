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
ROW_HEADER=$(printf 'program\tstate\tstage\tlatency_ns\tmemory_bytes\tvariance_pct\toutput_sha256:stderr_sha256\tphases')
SCRATCH_ROOT=${JET_PERF_SCRATCH_ROOT:-"$HOME/.cache/jet-perf"}
STATE_COUNT=6

# CI evidence must name the exact checked-out candidate. The explicit override
# is for CI wrappers; local runs fall back to the checked-out revision. A
# mismatched GitHub SHA is a stale checkout, not a usable performance result.
candidate_commit=${JET_CI_CANDIDATE_COMMIT:-${GITHUB_SHA:-}}
if [ -z "$candidate_commit" ]; then
    candidate_commit=$(git -C "$ROOT" rev-parse --verify HEAD 2>/dev/null || true)
fi
case "$candidate_commit" in
    ''|*[!0-9a-fA-F]*) echo "missing or invalid candidate commit identity" >&2; exit 1 ;;
esac
[ "${#candidate_commit}" -eq 40 ] || {
    echo "candidate commit identity must be a 40-character SHA-1" >&2
    exit 1
}
if [ -n "${GITHUB_SHA:-}" ] && [ "$candidate_commit" != "$GITHUB_SHA" ]; then
    echo "candidate commit does not match GITHUB_SHA: $candidate_commit != $GITHUB_SHA" >&2
    exit 1
fi
printf 'candidate_commit=%s\n' "$candidate_commit" >&2

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

baseline_schema=$(json_string schema)
baseline_corpus=$(json_string corpus_sha256)
baseline_manifest=$(json_string manifest_sha256)
baseline_version=$(json_number version)
baseline_stage=$(json_string stage)
baseline_os=$(json_string os)
baseline_arch=$(json_string arch)
baseline_target=$(json_string target)
baseline_rustc=$(json_string rustc)
baseline_llvm=$(json_string llvm)
baseline_rustc_vv=$(json_string rustc_vv_sha256)
baseline_rustc_sha=$(json_string rustc_sha256)
baseline_compiler=$(json_string compiler_sha256)
baseline_jet_env=$(json_string jet_env_sha256)
baseline_libc=$(json_string libc_sha256)
baseline_allocator=$(json_string allocator_source_sha256)
baseline_allocator_environment=$(json_string allocator_environment_sha256)
baseline_hardware=$(json_string hardware_sha256)
baseline_topology=$(json_string topology_sha256)
baseline_toolchain=$(json_string toolchain_sha256)
baseline_kernel=$(json_string kernel)
baseline_governor=$(json_string governor)
baseline_cpus=$(json_number cpus)
baseline_memory=$(json_number memory_bytes)
baseline_load_start=$(json_number load1_start_milli)
baseline_load_peak=$(json_number load1_peak_milli)
baseline_load_end=$(json_number load1_end_milli)
baseline_host=$(json_string hostname)
latency_budget=$(json_number latency_regression_pct)
memory_budget=$(json_number memory_regression_pct)
variance_budget=$(json_number variance_pct)
baseline_samples=$(json_number samples)
baseline_warmups=$(json_number warmups)
baseline_parity=$(json_string status)
baseline_parity_cases=$(json_number cases)
baseline_semantic_parity=$(json_string semantic)
baseline_diagnostic_parity=$(json_string diagnostics)
baseline_effect_parity=$(json_string effects)
baseline_tier_parity=$(json_string tiers)
[ -n "$baseline_schema" ] || { echo "baseline has no report schema" >&2; exit 1; }
[ "$baseline_schema" = "jet.compiler-speed" ] || {
    echo "unsupported compiler-speed baseline schema: $baseline_schema" >&2
    exit 1
}
[ -n "$baseline_version" ] || { echo "baseline has incomplete corpus/stage/machine/budget identity" >&2; exit 1; }
[ "$baseline_version" -eq 4 ] || { echo "unsupported compiler-speed baseline version: $baseline_version" >&2; exit 1; }
for value in "$baseline_corpus" "$baseline_manifest" "$baseline_stage" "$baseline_os" "$baseline_arch" "$baseline_target" "$baseline_rustc" "$baseline_llvm" "$baseline_rustc_vv" "$baseline_rustc_sha" "$baseline_compiler" "$baseline_jet_env" "$baseline_libc" "$baseline_allocator" "$baseline_allocator_environment" "$baseline_hardware" "$baseline_topology" "$baseline_toolchain" "$baseline_kernel" "$baseline_governor" "$baseline_cpus" "$baseline_memory" "$baseline_host" "$latency_budget" "$memory_budget" "$variance_budget" "$baseline_samples" "$baseline_warmups"; do
    [ -n "$value" ] || { echo "baseline has incomplete corpus/stage/machine/budget identity" >&2; exit 1; }
done
for value in "$baseline_load_start" "$baseline_load_peak" "$baseline_load_end"; do
    case "$value" in
        ''|*[!0-9]*) echo "baseline has invalid machine load accounting" >&2; exit 1 ;;
    esac
done
[ "$baseline_load_peak" -ge "$baseline_load_start" ] && [ "$baseline_load_peak" -ge "$baseline_load_end" ] || {
    echo "baseline has invalid machine load peak" >&2
    exit 1
}
for value in "$baseline_parity" "$baseline_parity_cases" "$baseline_semantic_parity" "$baseline_diagnostic_parity" "$baseline_effect_parity" "$baseline_tier_parity"; do
    [ -n "$value" ] || { echo "baseline has incomplete parity receipt" >&2; exit 1; }
done
[ "$baseline_parity" = verified ] && [ "$baseline_semantic_parity" = verified ] && \
    [ "$baseline_diagnostic_parity" = verified ] && [ "$baseline_effect_parity" = verified ] && \
    [ "$baseline_tier_parity" = verified ] || {
    echo "baseline has unverified parity receipt" >&2
    exit 1
}
case "$baseline_parity_cases" in
    ''|0|*[!0-9]*) echo "baseline has invalid parity case count" >&2; exit 1 ;;
esac

case "$THRESH" in
    "") latency_threshold=$latency_budget; memory_threshold=$memory_budget ;;
    *[!0-9]*) echo "threshold must be a non-negative integer" >&2; exit 2 ;;
    *) latency_threshold=$THRESH; memory_threshold=$THRESH ;;
esac

# Keep CI scratch off RAM-backed `/tmp` and out of compiler target trees. The
# dashboard and this checker share one explicit disk-backed root, then each
# invocation gets a unique private directory.
scratch_resolved=$(realpath -m -- "$SCRATCH_ROOT" 2>/dev/null || printf '%s' "$SCRATCH_ROOT")
case "$scratch_resolved" in
    /tmp|/tmp/*|*/target|*/target/*)
        echo "refusing compiler-speed CI scratch on RAM-backed /tmp or a target directory: $SCRATCH_ROOT" >&2
        exit 1
        ;;
esac
mkdir -p "$SCRATCH_ROOT"
scratch_device=$(df -P "$SCRATCH_ROOT" 2>/dev/null | awk 'NR == 2 { print $1; exit }')
case "$scratch_device" in
    ""|tmpfs|ramfs|none)
        echo "refusing compiler-speed CI scratch without a disk-backed filesystem: $SCRATCH_ROOT" >&2
        exit 1
        ;;
esac
CI_RUN_DIR=$(mktemp -d "$SCRATCH_ROOT/compiler-speed-ci.XXXXXX")
cleanup_ci() {
    cleanup_status=$?
    trap - EXIT HUP INT TERM
    rm -rf "$CI_RUN_DIR"
    exit "$cleanup_status"
}
trap cleanup_ci EXIT HUP INT TERM

# The production dashboard fails before publishing a report when its Tukey
# outlier gate trips; this assignment preserves that nonzero result.
CURRENT=$(JET_PERF_SCRATCH_ROOT="$SCRATCH_ROOT" TMPDIR="$SCRATCH_ROOT" "$PERF_DIR/dashboard.sh")
metadata=$(printf '%s\n' "$CURRENT" | sed -n '1p')
row_header=$(printf '%s\n' "$CURRENT" | sed -n '2p')
case "$metadata" in
    compiler-speed\ version=*) ;;
    *) echo "invalid compiler-speed report metadata" >&2; exit 1 ;;
esac
[ "$row_header" = "$ROW_HEADER" ] || {
    echo "invalid compiler-speed row header" >&2
    exit 1
}
current_version=$(printf '%s\n' "$metadata" | sed -n 's/.*version=\([^ ]*\).*/\1/p')
current_corpus_count=$(printf '%s\n' "$metadata" | sed -n 's/.*corpus=\([0-9][0-9]*\).*/\1/p')
current_corpus=$(printf '%s\n' "$metadata" | sed -n 's/.*corpus_sha256=\([^ ]*\).*/\1/p')
current_manifest=$(printf '%s\n' "$metadata" | sed -n 's/.*manifest_sha256=\([^ ]*\).*/\1/p')
current_stage=$(printf '%s\n' "$metadata" | sed -n 's/.*stage=\([^ ]*\).*/\1/p')
current_machine=$(printf '%s\n' "$metadata" | sed -n 's/.*machine=\([^ ]*\).*/\1/p')
current_target=$(printf '%s\n' "$metadata" | sed -n 's/.*target=\([^ ]*\).*/\1/p')
current_rustc=$(printf '%s\n' "$metadata" | sed -n 's/.*rustc=\([^ ]*\).*/\1/p')
current_llvm=$(printf '%s\n' "$metadata" | sed -n 's/.*llvm=\([^ ]*\).*/\1/p')
current_rustc_vv=$(printf '%s\n' "$metadata" | sed -n 's/.*rustc_vv_sha256=\([^ ]*\).*/\1/p')
current_rustc_sha=$(printf '%s\n' "$metadata" | sed -n 's/.*rustc_sha256=\([^ ]*\).*/\1/p')
current_compiler=$(printf '%s\n' "$metadata" | sed -n 's/.*compiler_sha256=\([^ ]*\).*/\1/p')
current_jet_env=$(printf '%s\n' "$metadata" | sed -n 's/.*jet_env_sha256=\([^ ]*\).*/\1/p')
current_libc=$(printf '%s\n' "$metadata" | sed -n 's/.*libc_sha256=\([^ ]*\).*/\1/p')
current_allocator=$(printf '%s\n' "$metadata" | sed -n 's/.*allocator_sha256=\([^ ]*\).*/\1/p')
current_allocator_environment=$(printf '%s\n' "$metadata" | sed -n 's/.*allocator_environment_sha256=\([^ ]*\).*/\1/p')
current_hardware=$(printf '%s\n' "$metadata" | sed -n 's/.*hardware_sha256=\([^ ]*\).*/\1/p')
current_topology=$(printf '%s\n' "$metadata" | sed -n 's/.*topology_sha256=\([^ ]*\).*/\1/p')
current_toolchain=$(printf '%s\n' "$metadata" | sed -n 's/.*toolchain_sha256=\([^ ]*\).*/\1/p')
current_kernel=$(printf '%s\n' "$metadata" | sed -n 's/.*kernel=\([^ ]*\).*/\1/p')
current_governor=$(printf '%s\n' "$metadata" | sed -n 's/.*governor=\([^ ]*\).*/\1/p')
current_load_start=$(printf '%s\n' "$metadata" | sed -n 's/.*load1_start_milli=\([^ ]*\).*/\1/p')
current_load_peak=$(printf '%s\n' "$metadata" | sed -n 's/.*load1_peak_milli=\([^ ]*\).*/\1/p')
current_load_end=$(printf '%s\n' "$metadata" | sed -n 's/.*load1_end_milli=\([^ ]*\).*/\1/p')
current_memory=$(printf '%s\n' "$metadata" | sed -n 's/.*memory_bytes=\([^ ]*\).*/\1/p')
current_samples=$(printf '%s\n' "$metadata" | sed -n 's/.*samples=\([^ ]*\).*/\1/p')
current_warmups=$(printf '%s\n' "$metadata" | sed -n 's/.*warmups=\([^ ]*\).*/\1/p')
current_parity=$(printf '%s\n' "$metadata" | sed -n 's/.*parity=\([^ ]*\).*/\1/p')
current_parity_cases=$(printf '%s\n' "$metadata" | sed -n 's/.*parity_cases=\([^ ]*\).*/\1/p')
current_os=$(printf '%s\n' "$current_machine" | cut -d/ -f1)
current_arch=$(printf '%s\n' "$current_machine" | cut -d/ -f2)
current_cpus=$(printf '%s\n' "$current_machine" | sed 's/.*cpus=\([^/]*\).*/\1/')
current_host=$(printf '%s\n' "$current_machine" | sed 's/.*host=//')

case "$current_corpus_count" in
    ''|0|*[!0-9]*) echo "invalid compiler-speed corpus count: $current_corpus_count" >&2; exit 1 ;;
esac
case "$current_parity_cases" in
    ''|0|*[!0-9]*) echo "invalid compiler-speed parity case count: $current_parity_cases" >&2; exit 1 ;;
esac
for value in "$current_load_start" "$current_load_peak" "$current_load_end"; do
    case "$value" in
        ''|*[!0-9]*) echo "invalid compiler-speed machine load accounting" >&2; exit 1 ;;
    esac
done
[ "$current_load_peak" -ge "$current_load_start" ] && [ "$current_load_peak" -ge "$current_load_end" ] || {
    echo "invalid compiler-speed machine load peak" >&2
    exit 1
}
[ "$current_parity" = verified ] || { echo "current report has unverified parity receipt" >&2; exit 1; }

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
check_identity package-manifest "$current_manifest" "$baseline_manifest"
check_identity report-version "$current_version" "$baseline_version"
check_identity stage "$current_stage" "$baseline_stage"
check_identity OS "$current_os" "$baseline_os"
check_identity architecture "$current_arch" "$baseline_arch"
check_identity target "$current_target" "$baseline_target"
check_identity rustc "$current_rustc" "$baseline_rustc"
check_identity LLVM "$current_llvm" "$baseline_llvm"
check_identity rustc-vV "$current_rustc_vv" "$baseline_rustc_vv"
check_identity rustc-binary "$current_rustc_sha" "$baseline_rustc_sha"
check_identity compiler "$current_compiler" "$baseline_compiler"
check_identity jet-env "$current_jet_env" "$baseline_jet_env"
check_identity libc "$current_libc" "$baseline_libc"
check_identity allocator "$current_allocator" "$baseline_allocator"
check_identity allocator-environment "$current_allocator_environment" "$baseline_allocator_environment"
check_identity hardware "$current_hardware" "$baseline_hardware"
check_identity hardware-topology "$current_topology" "$baseline_topology"
check_identity toolchain "$current_toolchain" "$baseline_toolchain"
check_identity kernel "$current_kernel" "$baseline_kernel"
check_identity governor "$current_governor" "$baseline_governor"
check_identity CPU-count "$current_cpus" "$baseline_cpus"
check_identity machine-memory "$current_memory" "$baseline_memory"
check_identity host "$current_host" "$baseline_host"
check_identity samples "$current_samples" "$baseline_samples"
check_identity warmups "$current_warmups" "$baseline_warmups"
check_identity parity-cases "$current_parity_cases" "$baseline_parity_cases"

baseline_field() {
    field_program=$1
    field_state=$2
    field_name=$3
    baseline_row=$(sed 's/},{/}\n{/g' "$BASELINE" \
        | grep -F '"program":"'"$field_program"'","state":"'"$field_state"'"' \
        | head -n1 || true)
    [ -n "$baseline_row" ] || return 0
    printf '%s\n' "$baseline_row" \
        | sed 's/.*"'"$field_name"'"://; s/^"//; s/".*//; s/,.*//; s/}.*//'
}

phase_value() {
    phase_text=$1
    phase_name=$2
    printf '%s\n' "$phase_text" | sed -n "s/.*;$phase_name=\\([^;]*\\).*/\\1/p"
}

FAIL=0
ROW_COUNT=0
CURRENT_ROWS="$CI_RUN_DIR/current.rows"
CURRENT_KEYS="$CI_RUN_DIR/current.keys"
printf '%s\n' "$CURRENT" | tail -n +3 > "$CURRENT_ROWS"
: > "$CURRENT_KEYS"
awk -F "$TAB" 'NF != 8 { exit 1 }' "$CURRENT_ROWS" || {
    echo "invalid compiler-speed row format" >&2
    exit 1
}
expected_rows=$((current_corpus_count * STATE_COUNT))
baseline_row_count=$(sed 's/},{/}\n{/g' "$BASELINE" \
    | awk '/"program":"[^"]*","state":"[^"]*"/ { count++ } END { print count + 0 }')
[ "$baseline_row_count" -eq "$expected_rows" ] || {
    echo "baseline row count changed: expected $expected_rows, got $baseline_row_count" >&2
    exit 1
}

# Rows use the canonical tab-separated fields named by ROW_HEADER.
while IFS="$TAB" read -r row_program row_state row_stage row_latency row_memory row_variance row_output row_phases; do
    [ -n "${row_program:-}" ] || continue
    ROW_COUNT=$((ROW_COUNT + 1))
    case "$row_latency:$row_memory:$row_variance" in
        ''|*[!0-9:]*|*::*|*:*:) echo "incomplete current timing row: $row_program/$row_state" >&2; exit 1 ;;
    esac
    if [ "$row_latency" -eq 0 ] || [ "$row_memory" -eq 0 ]; then
        echo "zero current timing row: $row_program/$row_state" >&2
        exit 1
    fi
    row_key=$(printf '%s\t%s' "$row_program" "$row_state")
    if grep -Fqx -- "$row_key" "$CURRENT_KEYS"; then
        echo "duplicate current timing row: $row_program/$row_state" >&2
        exit 1
    fi
    printf '%s\n' "$row_key" >> "$CURRENT_KEYS"
    base_stage=$(baseline_field "$row_program" "$row_state" stage)
    base_latency=$(baseline_field "$row_program" "$row_state" latency_ns)
    base_memory=$(baseline_field "$row_program" "$row_state" memory_bytes)
    base_variance=$(baseline_field "$row_program" "$row_state" variance_pct)
    base_stdout=$(baseline_field "$row_program" "$row_state" stdout_sha256)
    base_stderr=$(baseline_field "$row_program" "$row_state" stderr_sha256)
    base_phases=$(baseline_field "$row_program" "$row_state" phase_totals)
    for value in "$base_stage" "$base_latency" "$base_memory" "$base_variance" "$base_stdout" "$base_stderr" "$base_phases"; do
        [ -n "$value" ] || { echo "baseline missing row: $row_program/$row_state" >&2; exit 1; }
    done
    [ "$row_stage" = "$base_stage" ] || { echo "stage changed for $row_program/$row_state" >&2; exit 1; }
    case "$base_latency:$base_memory:$base_variance" in
        *[!0-9:]*|*::*|*:*:) echo "invalid baseline row: $row_program/$row_state" >&2; exit 1 ;;
    esac
    if [ "$base_latency" -eq 0 ] || [ "$base_memory" -eq 0 ]; then
        echo "zero baseline timing row: $row_program/$row_state" >&2
        exit 1
    fi
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
        echo "UNSTABLE $row_program/$row_state interquartile spread=${row_variance}% budget=${variance_budget}%" >&2
        FAIL=1
    fi
    row_stdout=${row_output%%:*}
    row_stderr=${row_output#*:}
    [ "$row_stdout" = "$base_stdout" ] || { echo "stdout parity changed for $row_program/$row_state" >&2; FAIL=1; }
    [ "$row_stderr" = "$base_stderr" ] || { echo "stderr parity changed for $row_program/$row_state" >&2; FAIL=1; }
    case "$row_phases" in
        *phases=*source=*source_sha256=*source_bytes=*expected_sha256=*expected_bytes=*manifest_sha256=*workload_sha256=*role=*profile=*backend=*linker=*linker_path=*linker_sha256=*linker_backend=*linker_backend_path=*linker_backend_sha256=*cache_state=*cache_policy=*cache_hits=*cache_misses=*libc_sha256=*allocator_sha256=*allocator_environment_sha256=*hardware_sha256=*topology_sha256=*toolchain_sha256=*rustc_sha256=*top_cause=*artifact_bytes=*) ;;
        *) echo "missing phase totals for $row_program/$row_state" >&2; FAIL=1 ;;
    esac
    case "$row_phases" in
        *linker=unavailable*) echo "missing linker identity for $row_program/$row_state" >&2; FAIL=1 ;;
    esac
    case "$row_phases" in
        *parity=verified*semantic_parity=verified*diagnostic_parity=verified*effect_parity=verified*tier_parity=verified*dev_profile=dev*aot_profile=release*) ;;
        *) echo "missing JIT/dev/AOT semantic parity receipt for $row_program/$row_state" >&2; FAIL=1 ;;
    esac
    for identity_field in workload_sha256 source_sha256 source_bytes expected_sha256 expected_bytes manifest_sha256 cache_state cache_policy backend linker linker_path linker_sha256 linker_backend linker_backend_path linker_backend_sha256 libc_sha256 allocator_sha256 allocator_environment_sha256 hardware_sha256 topology_sha256 toolchain_sha256 rustc_sha256; do
        current_identity=$(phase_value "$row_phases" "$identity_field")
        base_identity=$(phase_value "$base_phases" "$identity_field")
        if [ -z "$current_identity" ] || [ -z "$base_identity" ]; then
            echo "missing workload/environment identity $identity_field for $row_program/$row_state" >&2
            FAIL=1
        elif [ "$current_identity" != "$base_identity" ]; then
            echo "unmatched workload/environment identity $identity_field for $row_program/$row_state" >&2
            FAIL=1
        fi
    done
    for parity_field in parity semantic_parity diagnostic_parity effect_parity tier_parity dev_profile aot_profile; do
        current_parity_value=$(phase_value "$row_phases" "$parity_field")
        base_parity_value=$(phase_value "$base_phases" "$parity_field")
        if [ -z "$current_parity_value" ] || [ -z "$base_parity_value" ]; then
            echo "missing parity receipt $parity_field for $row_program/$row_state" >&2
            FAIL=1
        elif [ "$current_parity_value" != "$base_parity_value" ]; then
            echo "unmatched parity receipt $parity_field for $row_program/$row_state" >&2
            FAIL=1
        fi
    done
done < "$CURRENT_ROWS"

[ "$ROW_COUNT" -eq "$expected_rows" ] || {
    echo "checked corpus row count changed: expected $expected_rows, got $ROW_COUNT" >&2
    exit 1
}
[ "$FAIL" -eq 0 ] || { echo "perf gate FAILED" >&2; exit 1; }
echo "perf gate OK (candidate ${candidate_commit}, latency ${latency_threshold}%, memory ${memory_threshold}%, variance ${variance_budget}%, rows ${ROW_COUNT})"
