#!/usr/bin/env sh
# Compiler-speed corpus dashboard.
#
# One checked corpus, six production rows per program:
#   jit-clean, jit-no-change, jit-representative-edit,
#   aot-release-clean, aot-release-no-change, aot-release-representative-edit.
# Every row records wall latency, peak compiler RSS, deterministic output,
# elapsed variance, and the phase report emitted by that production path.
#
# Usage:
#   tools/perf/dashboard.sh
#   tools/perf/dashboard.sh --baseline
#   tools/perf/dashboard.sh --compare FILE

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PERF_DIR="$ROOT/tools/perf"
CORPUS="$PERF_DIR/corpus.tsv"
BASELINE="$PERF_DIR/baseline.json"
TMP_ROOT=${TMPDIR:-"$HOME/.cache/jet-test-scratch"}
JET_ENV="$ROOT/scripts/agent/jet-env"
JET_BIN="$ROOT/target/debug/jet"
TIME_BIN=${TIME_BIN:-}
if [ -z "$TIME_BIN" ]; then
    TIME_BIN=$(type -P time 2>/dev/null || true)
fi
SAMPLES=20
WARMUPS=1
LATENCY_REGRESSION_PCT=15
MEMORY_REGRESSION_PCT=15
VARIANCE_BUDGET_PCT=100
TAB=$(printf '\t')

[ -x "$TIME_BIN" ] || { echo "missing GNU time: $TIME_BIN" >&2; exit 1; }
[ -f "$CORPUS" ] || { echo "missing compiler-speed corpus: $CORPUS" >&2; exit 1; }
[ -x "$ROOT/target/debug/jet" ] || {
    echo "missing fresh compiler binary: $ROOT/target/debug/jet" >&2
    echo "build Jet before running the compiler-speed dashboard" >&2
    exit 1
}
mkdir -p "$TMP_ROOT"

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

sha256_text() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | awk '{print $1}'
    else
        shasum -a 256 | awk '{print $1}'
    fi
}

json_q() {
    json_value=$1
    json_value=$(printf '%s' "$json_value" | sed 's/\\/\\\\/g; s/"/\\"/g')
    printf '"%s"' "$json_value"
}

require_relative_file() {
    case "$1" in
        ""|/*|*".."*)
            echo "invalid corpus path: $1" >&2
            exit 1
            ;;
    esac
    [ -f "$ROOT/$1" ] || { echo "missing corpus file: $1" >&2; exit 1; }
}

machine_os=$(uname -s 2>/dev/null || echo unknown)
machine_arch=$(uname -m 2>/dev/null || echo unknown)
machine_cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo unknown)
machine_host=$(hostname 2>/dev/null || echo unknown)
machine_kernel=$(uname -r 2>/dev/null || echo unknown)
machine_target=$(
    "$JET_ENV" rustc -vV 2>/dev/null \
        | sed -n 's/^host: //p' \
        | head -n1
)
machine_rustc=$(
    "$JET_ENV" rustc -vV 2>/dev/null \
        | sed -n 's/^release: //p' \
        | head -n1
)
machine_llvm=$(
    "$JET_ENV" rustc -vV 2>/dev/null \
        | sed -n 's/^LLVM version: //p' \
        | head -n1
)
machine_rustc_vv_sha=$("$JET_ENV" rustc -vV 2>/dev/null | sha256_text)
machine_memory=$(awk '/^MemTotal:/ { print $2 * 1024; exit }' /proc/meminfo 2>/dev/null || true)
machine_governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo unknown)
compiler_sha256=$(sha256 "$ROOT/target/debug/jet")

case "$machine_cpus" in
    ''|*[!0-9]*) echo "unavailable machine CPU identity: $machine_cpus" >&2; exit 1 ;;
esac
case "$machine_memory" in
    ''|*[!0-9]*) echo "unavailable machine memory identity" >&2; exit 1 ;;
esac
for identity_value in "$machine_host" "$machine_target" "$machine_rustc" "$machine_llvm" "$machine_rustc_vv_sha" "$machine_kernel" "$machine_governor"; do
    case "$identity_value" in
        ""|*'"'*) echo "unavailable machine/toolchain identity" >&2; exit 1 ;;
    esac
done
machine="$machine_os/$machine_arch/cpus=$machine_cpus/host=$machine_host"
corpus_sha=$(sha256 "$CORPUS")
run_dir=$(mktemp -d "$TMP_ROOT/compiler-speed.XXXXXX")
rows_file="$run_dir/rows.tsv"
outputs_dir="$run_dir/outputs"
mkdir -p "$outputs_dir"
trap 'rm -rf "$run_dir"' EXIT HUP INT TERM

check_corpus() {
    corpus_count=0
    while IFS="$TAB" read -r corpus_program corpus_expected corpus_source_hash corpus_expected_hash corpus_edit corpus_edit_expected corpus_edit_hash corpus_edit_expected_hash; do
        case "$corpus_program" in
            ""|\#*) continue ;;
        esac
        if [ -z "${corpus_expected:-}" ] || [ -z "${corpus_source_hash:-}" ] || [ -z "${corpus_expected_hash:-}" ] || \
            [ -z "${corpus_edit:-}" ] || [ -z "${corpus_edit_expected:-}" ] || \
            [ -z "${corpus_edit_hash:-}" ] || [ -z "${corpus_edit_expected_hash:-}" ]; then
            echo "malformed corpus row: $corpus_program" >&2
            exit 1
        fi
        require_relative_file "$corpus_program"
        require_relative_file "$corpus_expected"
        require_relative_file "$corpus_edit"
        require_relative_file "$corpus_edit_expected"
        actual_hash=$(sha256 "$ROOT/$corpus_program")
        [ "$actual_hash" = "$corpus_source_hash" ] || {
            echo "corpus source changed: $corpus_program ($corpus_source_hash -> $actual_hash)" >&2
            exit 1
        }
        actual_hash=$(sha256 "$ROOT/$corpus_expected")
        [ "$actual_hash" = "$corpus_expected_hash" ] || {
            echo "corpus golden changed: $corpus_expected ($corpus_expected_hash -> $actual_hash)" >&2
            exit 1
        }
        actual_hash=$(sha256 "$ROOT/$corpus_edit")
        [ "$actual_hash" = "$corpus_edit_hash" ] || {
            echo "representative edit changed: $corpus_edit ($corpus_edit_hash -> $actual_hash)" >&2
            exit 1
        }
        actual_hash=$(sha256 "$ROOT/$corpus_edit_expected")
        [ "$actual_hash" = "$corpus_edit_expected_hash" ] || {
            echo "representative edit golden changed: $corpus_edit_expected ($corpus_edit_expected_hash -> $actual_hash)" >&2
            exit 1
        }
        [ "$corpus_source_hash" != "$corpus_edit_hash" ] || {
            echo "representative edit is unchanged: $corpus_program -> $corpus_edit" >&2
            exit 1
        }
        corpus_count=$((corpus_count + 1))
    done < "$CORPUS"
    [ "$corpus_count" -gt 0 ] || { echo "empty compiler-speed corpus" >&2; exit 1; }
    echo "$corpus_count"
}

timing_field() {
    timing_file=$1
    timing_name=$2
    [ -s "$timing_file" ] || return 0
    sed 's/},{/}\n{/g' "$timing_file" \
        | grep '"name":"'"$timing_name"'"' \
        | sed 's/.*"us"://; s/[^0-9].*//' \
        | head -n1
}

append_timing_phases() {
    timing_file=$1
    phase_file=$2
    [ -s "$timing_file" ] || { echo "missing timing report: $timing_file" >&2; exit 1; }
    for phase_name in load sema ffi codegen build_plan backend_link frontend jit jit_cache_hit cache_hit rust_bytes; do
        phase_value=$(timing_field "$timing_file" "$phase_name")
        case "$phase_value" in
            ""|*[!0-9]*) ;;
            *) printf '%s%s%s\n' "$phase_name" "$TAB" "$phase_value" >> "$phase_file" ;;
        esac
    done
}

append_cache_miss() {
    phase_file=$1
    cache_hit_value=$2
    case "$cache_hit_value" in
        1) printf '%s%s%s\n' cache_miss "$TAB" 0 >> "$phase_file" ;;
        *) printf '%s%s%s\n' cache_miss "$TAB" 1 >> "$phase_file" ;;
    esac
}

top_cause() {
    phase_file_path=$1
    awk -F "$TAB" '
        $1 !~ /^(rust_bytes|cache_hit|cache_miss)$/ && $2 ~ /^[0-9]+$/ && $2 > maximum {
            maximum = $2
            name = $1
        }
        END {
            if (name == "") print "unavailable"
            else print name "=" maximum "us"
        }
    ' "$phase_file_path"
}

run_timed() {
    timed_stats=$1
    timed_stdout=$2
    timed_stderr=$3
    shift 3
    "$TIME_BIN" -f '%e\t%M' -o "$timed_stats" "$@" >"$timed_stdout" 2>"$timed_stderr"
}

read_timed_stats() {
    stats_file=$1
    IFS="$TAB" read -r elapsed_seconds peak_rss_kib < "$stats_file"
    case "$elapsed_seconds:$peak_rss_kib" in
        ''|*[!0-9.]*:*|*:*[!0-9]*)
            echo "incomplete process timing: $stats_file" >&2
            exit 1
            ;;
    esac
    TRIAL_LATENCY_NS=$(awk -v seconds="$elapsed_seconds" 'BEGIN { printf "%.0f\n", seconds * 1000000000 }')
    TRIAL_MEMORY_BYTES=$((peak_rss_kib * 1024))
    case "$TRIAL_LATENCY_NS:$TRIAL_MEMORY_BYTES" in
        ''|*[!0-9]*:*) echo "invalid process timing: $stats_file" >&2; exit 1 ;;
    esac
}

prepare_fixture() {
    fixture_work=$1
    fixture_program=$2
    fixture_expected=$3
    mkdir -p "$fixture_work"
    cp "$ROOT/$fixture_program" "$fixture_work/run.jet"
    cp "$ROOT/$fixture_expected" "$fixture_work/expected.out"
}

check_trial_output() {
    trial_work=$1
    trial_stdout=$2
    trial_stderr=$3
    cmp "$trial_work/expected.out" "$trial_stdout" || {
        echo "output mismatch: $trial_work/run.jet" >&2
        diff -u "$trial_work/expected.out" "$trial_stdout" >&2 || true
        exit 1
    }
    TRIAL_STDOUT_SHA256=$(sha256 "$trial_stdout")
    TRIAL_STDERR_SHA256=$(sha256 "$trial_stderr")
}

run_jit_trial() {
    trial_work=$1
    trial_cache=$2
    trial_output=$3
    trial_stats=$4
    trial_phases=$5
    mkdir -p "$trial_work/timing" "$trial_cache"
    if run_timed "$trial_stats" "$trial_output.stdout" "$trial_output.stderr" \
        env \
        JET_RUN_CACHE_DIR="$trial_cache/run" \
        JET_CACHE_DIR="$trial_cache/build" \
        JET_TIMING=1 \
        JET_TIMING_DIR="$trial_work/timing" \
        NO_COLOR=1 \
        bash -c "cd '$trial_work' && exec '$JET_BIN' run run.jet"; then
        trial_status=0
    else
        trial_status=$?
    fi
    [ "$trial_status" -eq 0 ] || {
        echo "JIT run failed: $trial_work/run.jet (exit $trial_status)" >&2
        sed -n '1,120p' "$trial_output.stderr" >&2 || true
        exit 1
    }
    check_trial_output "$trial_work" "$trial_output.stdout" "$trial_output.stderr"
    trial_timing_file="$trial_work/timing/jet-timing.json"
    append_timing_phases "$trial_timing_file" "$trial_phases"
    trial_cache_hit=$(timing_field "$trial_timing_file" cache_hit)
    case "$trial_cache_hit" in
        1) ;;
        *) trial_cache_hit=0 ;;
    esac
    append_cache_miss "$trial_phases" "$trial_cache_hit"
    read_timed_stats "$trial_stats"
}

run_aot_trial() {
    trial_work=$1
    trial_cache=$2
    trial_output=$3
    trial_stats=$4
    trial_phases=$5
    mkdir -p "$trial_work/timing" "$trial_cache"
    if run_timed "$trial_stats" "$trial_output.build.stdout" "$trial_output.build.stderr" \
        env \
        JET_CACHE_DIR="$trial_cache/build" \
        JET_TIMING=1 \
        JET_TIMING_DIR="$trial_work/timing" \
        NO_COLOR=1 \
        "$JET_ENV" bash -c "cd '$trial_work' && exec jet build --release --verbose run.jet"; then
        trial_status=0
    else
        trial_status=$?
    fi
    [ "$trial_status" -eq 0 ] || {
        echo "optimized AOT build failed: $trial_work/run.jet (exit $trial_status)" >&2
        sed -n '1,120p' "$trial_output.build.stderr" >&2 || true
        exit 1
    }
    [ -x "$trial_work/build/run" ] || { echo "missing AOT artifact: $trial_work/build/run" >&2; exit 1; }
    if "$trial_work/build/run" >"$trial_output.stdout" 2>"$trial_output.stderr"; then
        trial_status=0
    else
        trial_status=$?
    fi
    [ "$trial_status" -eq 0 ] || {
        echo "optimized AOT artifact failed: $trial_work/run.jet (exit $trial_status)" >&2
        sed -n '1,120p' "$trial_output.stderr" >&2 || true
        exit 1
    }
    check_trial_output "$trial_work" "$trial_output.stdout" "$trial_output.stderr"
    append_timing_phases "$trial_work/timing/jet-timing.json" "$trial_phases"
    append_timing_phases "$trial_work/timing/build/jet-timing-backend.json" "$trial_phases"
    aot_cache_hits=$(grep -c 'cache hit' "$trial_output.build.stderr" || true)
    aot_cache_misses=$(grep -c 'cache miss' "$trial_output.build.stderr" || true)
    printf '%s%s%s\n' cache_hit "$TAB" "$aot_cache_hits" >> "$trial_phases"
    printf '%s%s%s\n' cache_miss "$TAB" "$aot_cache_misses" >> "$trial_phases"
    TRIAL_LINKER=$(sed -n 's/.*\[build\] linker[[:space:]]*->[[:space:]]*//p' "$trial_output.build.stderr" | tail -n1)
    TRIAL_LINKER=${TRIAL_LINKER:-unavailable}
    TRIAL_ARTIFACT_BYTES=$(wc -c < "$trial_work/build/run" | tr -d ' ')
    read_timed_stats "$trial_stats"
}

median_file() {
    median_file_path=$1
    sort -n "$median_file_path" | awk -v count="$SAMPLES" 'NR == int((count + 1) / 2) { print $1; exit }'
}

variance_file() {
    variance_file_path=$1
    sort -n "$variance_file_path" | awk -v count="$SAMPLES" '
        { values[NR] = $1; if (NR == 1) minimum = $1; maximum = $1 }
        END {
            median = values[int((count + 1) / 2)]
            if (count == 0 || median == 0) exit 1
            printf "%.0f\n", ((maximum - minimum) * 100) / median
        }'
}

phase_average() {
    phase_file_path=$1
    phase_wanted=$2
    awk -F "$TAB" -v wanted="$phase_wanted" '$1 == wanted { total += $2; count++ } END { if (count == 0) print 0; else printf "%.0f\n", total / count }' "$phase_file_path"
}

safe_id() {
    printf '%s' "$1" | tr '/.' '__'
}

row_field() {
    row_field_program=$1
    row_field_state=$2
    row_field_number=$3
    awk -F "$TAB" -v program="$row_field_program" -v state="$row_field_state" -v field="$row_field_number" '$1 == program && $2 == state { print $field; exit }' "$rows_file"
}

measure_state() {
    state_program=$1
    state_expected=$2
    state_edit_program=$3
    state_edit_expected=$4
    state_name=$5
    state_stage=$6
    state_kind=$7
    state_id="$(safe_id "$state_program-$state_name")"
    state_root="$run_dir/$state_id"
    state_latency="$state_root/latency.ns"
    state_memory="$state_root/memory.bytes"
    state_phases="$state_root/phases.tsv"
    state_reference_stdout="$outputs_dir/$state_id.stdout"
    state_reference_stderr="$outputs_dir/$state_id.stderr"
    mkdir -p "$state_root"
    : > "$state_latency"
    : > "$state_memory"
    : > "$state_phases"

    if [ "$state_kind" = "clean" ]; then
        state_work="$state_root/warmup"
        state_cache="$state_work/cache"
        prepare_fixture "$state_work" "$state_program" "$state_expected"
        if [ "$state_stage" = "jit-fast" ]; then
            run_jit_trial "$state_work" "$state_cache" "$state_root/warmup" "$state_root/warmup.stats" "$state_phases"
        else
            run_aot_trial "$state_work" "$state_cache" "$state_root/warmup" "$state_root/warmup.stats" "$state_phases"
        fi
    else
        state_work="$state_root/seeded"
        state_cache="$state_work/cache"
        prepare_fixture "$state_work" "$state_program" "$state_expected"
        if [ "$state_stage" = "jit-fast" ]; then
            run_jit_trial "$state_work" "$state_cache" "$state_root/warmup" "$state_root/warmup.stats" "$state_phases"
        else
            run_aot_trial "$state_work" "$state_cache" "$state_root/warmup" "$state_root/warmup.stats" "$state_phases"
        fi
        if [ "$state_kind" = "edit" ]; then
            prepare_fixture "$state_work" "$state_edit_program" "$state_edit_expected"
        fi
    fi
    # Warmup proves that the path can run. Phase totals below describe only
    # the measured samples, so cache priming cannot dilute their timing.
    : > "$state_phases"

    sample_index=1
    while [ "$sample_index" -le "$SAMPLES" ]; do
        if [ "$state_kind" = "clean" ]; then
            sample_work="$state_root/sample-$sample_index"
            sample_cache="$sample_work/cache"
            prepare_fixture "$sample_work" "$state_program" "$state_expected"
        else
            sample_work="$state_work"
            sample_cache="$state_cache"
        fi
        sample_output="$state_root/sample-$sample_index"
        sample_stats="$state_root/sample-$sample_index.stats"
        if [ "$state_stage" = "jit-fast" ]; then
            run_jit_trial "$sample_work" "$sample_cache" "$sample_output" "$sample_stats" "$state_phases"
        else
            run_aot_trial "$sample_work" "$sample_cache" "$sample_output" "$sample_stats" "$state_phases"
        fi
        if [ "$sample_index" -eq 1 ]; then
            cp "$sample_output.stdout" "$state_reference_stdout"
            cp "$sample_output.stderr" "$state_reference_stderr"
        else
            cmp "$state_reference_stdout" "$sample_output.stdout" || { echo "nondeterministic stdout: $state_program/$state_name" >&2; exit 1; }
            cmp "$state_reference_stderr" "$sample_output.stderr" || { echo "nondeterministic stderr: $state_program/$state_name" >&2; exit 1; }
        fi
        printf '%s\n' "$TRIAL_LATENCY_NS" >> "$state_latency"
        printf '%s\n' "$TRIAL_MEMORY_BYTES" >> "$state_memory"
        sample_index=$((sample_index + 1))
    done

    state_latency_median=$(median_file "$state_latency")
    state_memory_max=$(sort -n "$state_memory" | tail -n1)
    state_variance=$(variance_file "$state_latency")
    if [ "$state_stage" = "jit-fast" ]; then
        state_backend="cranelift"
        state_linker="none"
        state_profile="fast"
        state_artifact_bytes=0
        state_phase_text="frontend_us=$(phase_average "$state_phases" frontend);jit_us=$(phase_average "$state_phases" jit);jit_cache_hit=$(phase_average "$state_phases" jit_cache_hit)"
    else
        state_backend="rustc-llvm"
        state_linker="$TRIAL_LINKER"
        state_profile="release"
        state_artifact_bytes="$TRIAL_ARTIFACT_BYTES"
        state_phase_text="load_us=$(phase_average "$state_phases" load);sema_us=$(phase_average "$state_phases" sema);ffi_us=$(phase_average "$state_phases" ffi);codegen_us=$(phase_average "$state_phases" codegen);build_plan_us=$(phase_average "$state_phases" build_plan);backend_link_us=$(phase_average "$state_phases" backend_link)"
    fi
    state_artifact_source="$state_work"
    if [ "$state_kind" = "clean" ]; then
        state_artifact_source="$state_root/sample-$SAMPLES"
    fi
    state_full_artifact="none"
    if [ -n "${JET_PERF_ARTIFACT_DIR:-}" ]; then
        state_full_artifact="$JET_PERF_ARTIFACT_DIR/$state_id"
        mkdir -p "$state_full_artifact"
        cp "$state_artifact_source/run.jet" "$state_full_artifact/run.jet"
        cp "$state_artifact_source/expected.out" "$state_full_artifact/expected.out"
        if [ -f "$state_artifact_source/timing/jet-timing.json" ]; then
            cp "$state_artifact_source/timing/jet-timing.json" "$state_full_artifact/jet-timing.json"
        fi
        if [ -f "$state_artifact_source/timing/build/jet-timing-backend.json" ]; then
            mkdir -p "$state_full_artifact/build"
            cp "$state_artifact_source/timing/build/jet-timing-backend.json" "$state_full_artifact/build/jet-timing-backend.json"
        fi
        if [ -f "$state_artifact_source/build/run.rs" ]; then
            mkdir -p "$state_full_artifact/build"
            cp "$state_artifact_source/build/run.rs" "$state_full_artifact/build/run.rs"
        fi
        if [ -x "$state_artifact_source/build/run" ]; then
            mkdir -p "$state_full_artifact/build"
            cp "$state_artifact_source/build/run" "$state_full_artifact/build/run"
        fi
    fi
    state_role=base
    state_source="$state_program"
    state_expected_source="$state_expected"
    if [ "$state_kind" = "edit" ]; then
        state_role=representative-edit
        state_source="$state_edit_program"
        state_expected_source="$state_edit_expected"
    fi
    state_source_hash=$(sha256 "$ROOT/$state_source")
    state_expected_hash=$(sha256 "$ROOT/$state_expected_source")
    state_phase_text="$state_phase_text;source=$state_source;source_sha256=$state_source_hash;expected_sha256=$state_expected_hash;role=$state_role;profile=$state_profile;backend=$state_backend;linker=$state_linker;cache_hits=$(phase_average "$state_phases" cache_hit);cache_misses=$(phase_average "$state_phases" cache_miss);generated_rust_bytes=$(phase_average "$state_phases" rust_bytes);artifact_bytes=$state_artifact_bytes;top_cause=$(top_cause "$state_phases");full_artifact=$state_full_artifact"
    printf '%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s\n' \
        "$state_program" "$TAB" "$state_name" "$TAB" "$state_stage" "$TAB" \
        "$state_latency_median" "$TAB" "$state_memory_max" "$TAB" "$state_variance" "$TAB" \
        "$(sha256 "$state_reference_stdout")" "$TAB" "$(sha256 "$state_reference_stderr")" "$TAB" \
        "$state_phase_text" >> "$rows_file"
}

corpus_count=$(check_corpus)
: > "$rows_file"
while IFS="$TAB" read -r program expected source_hash expected_hash edit_program edit_expected edit_hash edit_expected_hash; do
    case "$program" in
        ""|\#*) continue ;;
    esac
    measure_state "$program" "$expected" "$edit_program" "$edit_expected" "jit-clean" "jit-fast" "clean"
    measure_state "$program" "$expected" "$edit_program" "$edit_expected" "jit-no-change" "jit-fast" "no-change"
    measure_state "$program" "$expected" "$edit_program" "$edit_expected" "jit-representative-edit" "jit-fast" "edit"
    measure_state "$program" "$expected" "$edit_program" "$edit_expected" "aot-release-clean" "aot-release" "clean"
    measure_state "$program" "$expected" "$edit_program" "$edit_expected" "aot-release-no-change" "aot-release" "no-change"
    measure_state "$program" "$expected" "$edit_program" "$edit_expected" "aot-release-representative-edit" "aot-release" "edit"
    jit_clean_latency=$(row_field "$program" "jit-clean" 4)
    jit_no_change_latency=$(row_field "$program" "jit-no-change" 4)
    aot_clean_latency=$(row_field "$program" "aot-release-clean" 4)
    aot_no_change_latency=$(row_field "$program" "aot-release-no-change" 4)
    if [ "$jit_no_change_latency" -gt "$jit_clean_latency" ]; then
        echo "no-change JIT slower than clean: $program ($jit_clean_latency -> $jit_no_change_latency ns)" >&2
        exit 1
    fi
    if [ "$aot_no_change_latency" -gt "$aot_clean_latency" ]; then
        echo "no-change AOT slower than clean: $program ($aot_clean_latency -> $aot_no_change_latency ns)" >&2
        exit 1
    fi
    for scenario in clean no-change representative-edit; do
        jit_state="jit-$scenario"
        aot_state="aot-release-$scenario"
        jit_output_id="$(safe_id "$program-$jit_state")"
        aot_output_id="$(safe_id "$program-$aot_state")"
        jit_stdout="$outputs_dir/$jit_output_id.stdout"
        jit_stderr="$outputs_dir/$jit_output_id.stderr"
        aot_stdout="$outputs_dir/$aot_output_id.stdout"
        aot_stderr="$outputs_dir/$aot_output_id.stderr"
        [ -s "$jit_stdout" ] || { echo "missing parity output: $program/$jit_state" >&2; exit 1; }
        [ -s "$aot_stdout" ] || { echo "missing parity output: $program/$aot_state" >&2; exit 1; }
        cmp "$jit_stdout" "$aot_stdout" || { echo "JIT/AOT stdout parity failed: $program/$scenario" >&2; exit 1; }
        cmp "$jit_stderr" "$aot_stderr" || { echo "JIT/AOT stderr parity failed: $program/$scenario" >&2; exit 1; }
    done
done < "$CORPUS"

print_table() {
    printf 'compiler-speed corpus=%s corpus_sha256=%s stage=matrix machine=%s target=%s rustc=%s llvm=%s rustc_vv_sha256=%s compiler_sha256=%s kernel=%s governor=%s memory_bytes=%s profiles=jit-fast,aot-release backends=cranelift,rustc-llvm warmups=%s samples=%s\n' \
        "$corpus_count" "$corpus_sha" "$machine" "$machine_target" "$machine_rustc" \
        "$machine_llvm" "$machine_rustc_vv_sha" "$compiler_sha256" "$machine_kernel" "$machine_governor" "$machine_memory" "$WARMUPS" "$SAMPLES"
    printf '%-54s %-25s %-16s %16s %16s %10s %-64s\n' \
        program state stage latency_ns memory_bytes variance_pct output_sha256:stderr_sha256
    while IFS="$TAB" read -r row_program row_state row_stage row_latency row_memory row_variance row_stdout_sha row_stderr_sha row_phases; do
        printf '%-54s %-25s %-16s %16s %16s %10s %s:%s phases=%s\n' \
            "$row_program" "$row_state" "$row_stage" "$row_latency" "$row_memory" "$row_variance" \
            "$row_stdout_sha" "$row_stderr_sha" "$row_phases"
    done < "$rows_file"
}

as_json() {
    printf '{"schema":"jet.compiler-speed","version":3,"corpus_sha256":%s,"stage":"matrix",' "$(json_q "$corpus_sha")"
    printf '"machine":{"arch":%s,"compiler_sha256":%s,"cpus":%s,"governor":%s,"hostname":%s,"kernel":%s,"llvm":%s,"memory_bytes":%s,"os":%s,"rustc":%s,"rustc_vv_sha256":%s,"target":%s},' \
        "$(json_q "$machine_arch")" "$(json_q "$compiler_sha256")" "$machine_cpus" \
        "$(json_q "$machine_governor")" "$(json_q "$machine_host")" "$(json_q "$machine_kernel")" \
        "$(json_q "$machine_llvm")" "$machine_memory" "$(json_q "$machine_os")" "$(json_q "$machine_rustc")" \
        "$(json_q "$machine_rustc_vv_sha")" "$(json_q "$machine_target")"
    printf '"budgets":{"latency_regression_pct":%s,"memory_regression_pct":%s,"samples":%s,"variance_pct":%s,"warmups":%s},"runs":[' \
        "$LATENCY_REGRESSION_PCT" "$MEMORY_REGRESSION_PCT" "$SAMPLES" "$VARIANCE_BUDGET_PCT" "$WARMUPS"
    first=1
    while IFS="$TAB" read -r row_program row_state row_stage row_latency row_memory row_variance row_stdout_sha row_stderr_sha row_phases; do
        [ "$first" -eq 1 ] || printf ','
        first=0
        printf '{"program":%s,"state":%s,"stage":%s,"latency_ns":%s,"memory_bytes":%s,"variance_pct":%s,"stdout_sha256":%s,"stderr_sha256":%s,"phase_totals":%s}' \
            "$(json_q "$row_program")" "$(json_q "$row_state")" "$(json_q "$row_stage")" \
            "$row_latency" "$row_memory" "$row_variance" "$(json_q "$row_stdout_sha")" \
            "$(json_q "$row_stderr_sha")" "$(json_q "$row_phases")"
    done < "$rows_file"
    printf ']}\n'
}

case "${1:-}" in
    "")
        print_table
        ;;
    --baseline)
        as_json > "$BASELINE"
        print_table
        echo "baseline written to $BASELINE"
        ;;
    --compare)
        baseline_file=${2:-$BASELINE}
        [ -f "$baseline_file" ] || { echo "missing baseline: $baseline_file" >&2; exit 1; }
        print_table
        echo "baseline=$baseline_file"
        sed -n '1,120p' "$baseline_file"
        ;;
    *)
        echo "usage: tools/perf/dashboard.sh [--baseline|--compare FILE]" >&2
        exit 2
        ;;
esac
