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
#   tools/perf/dashboard.sh --json
#   tools/perf/dashboard.sh --baseline
#   tools/perf/dashboard.sh --compare FILE
#   tools/perf/dashboard.sh --environment
#   tools/perf/dashboard.sh --construct-scale
#   tools/perf/dashboard.sh --construct-scale-json

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PERF_DIR="$ROOT/tools/perf"
CORPUS="$PERF_DIR/corpus.tsv"
FIXTURE_PACKAGE="$PERF_DIR/package.jet"
SCALE_CORPUS="$PERF_DIR/construct-scale.tsv"
BASELINE="$PERF_DIR/baseline.json"
TMP_ROOT=${TMPDIR:-"$HOME/.cache/jet-test-scratch"}
JET_ENV="$ROOT/scripts/agent/jet-env"
JET_BIN="$ROOT/target/debug/jet"

# Materialize the core toolchain once. Re-entering `jet-env` for every AOT
# sample adds an environment-launch measurement to the compiler stage and
# makes clean samples bimodal; the measured command must be the same production
# compiler inside the already-pinned shell.
if [ "${JET_ENV_DISABLE:-0}" != "1" ]; then
    exec "$JET_ENV" "$ROOT/tools/perf/dashboard.sh" "$@"
fi

TIME_BIN=${TIME_BIN:-}
if [ -z "$TIME_BIN" ]; then
    TIME_BIN=$(type -P time 2>/dev/null || true)
fi
TIMEOUT_BIN=${TIMEOUT_BIN:-}
if [ -z "$TIMEOUT_BIN" ]; then
    TIMEOUT_BIN=$(type -P timeout 2>/dev/null || true)
fi
SAMPLES=20
SCALE_SAMPLES=${JET_PERF_SCALE_SAMPLES:-3}
WARMUPS=1
TRIAL_DEADLINE_SECONDS=120
MAX_CORPUS_FILE_BYTES=$((64 * 1024 * 1024))
LATENCY_REGRESSION_PCT=15
MEMORY_REGRESSION_PCT=15
VARIANCE_BUDGET_PCT=100
TAB=$(printf '\t')
REPORT_VERSION=4
PARITY_RECEIPT=unverified
PARITY_CASE_COUNT=0

[ -x "$TIME_BIN" ] || { echo "missing GNU time: $TIME_BIN" >&2; exit 1; }
[ -x "$TIMEOUT_BIN" ] || { echo "missing timeout: $TIMEOUT_BIN" >&2; exit 1; }
[ -f "$CORPUS" ] || { echo "missing compiler-speed corpus: $CORPUS" >&2; exit 1; }
[ -f "$FIXTURE_PACKAGE" ] || { echo "missing compiler-speed fixture package: $FIXTURE_PACKAGE" >&2; exit 1; }
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

file_bytes() {
    wc -c < "$1" | tr -d '[:space:]'
}

resolve_tool_path() {
    tool_name=$1
    case "$tool_name" in
        /*) tool_path=$tool_name ;;
        *) tool_path=$("$JET_ENV" sh -c 'command -v "$1"' sh "$tool_name" 2>/dev/null || true) ;;
    esac
    [ -n "$tool_path" ] || return 1
    [ -f "$tool_path" ] || return 1
    readlink -f "$tool_path" 2>/dev/null || printf '%s\n' "$tool_path"
}

require_identity() {
    identity_name=$1
    identity_value=$2
    case "$identity_value" in
        ""|*'"'*|*'\n'*)
            echo "unavailable $identity_name identity" >&2
            exit 1
            ;;
    esac
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

check_corpus_file_size() {
    corpus_path=$1
    corpus_bytes=$(wc -c < "$ROOT/$corpus_path" | tr -d '[:space:]')
    case "$corpus_bytes" in
        ''|*[!0-9]*) echo "unavailable corpus file size: $corpus_path" >&2; exit 1 ;;
    esac
    [ "$corpus_bytes" -le "$MAX_CORPUS_FILE_BYTES" ] || {
        echo "pathological corpus input exceeds ${MAX_CORPUS_FILE_BYTES} bytes: $corpus_path ($corpus_bytes)" >&2
        exit 1
    }
}

machine_os=$(uname -s 2>/dev/null || echo unknown)
machine_arch=$(uname -m 2>/dev/null || echo unknown)
machine_cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo unknown)
machine_host=$(hostname 2>/dev/null || echo unknown)
machine_kernel=$(uname -r 2>/dev/null || echo unknown)
machine_rustc_vv=$("$JET_ENV" rustc -vV 2>/dev/null || true)
machine_target=$(printf '%s\n' "$machine_rustc_vv" | sed -n 's/^host: //p' | head -n1)
machine_rustc=$(printf '%s\n' "$machine_rustc_vv" | sed -n 's/^release: //p' | head -n1)
machine_llvm=$(printf '%s\n' "$machine_rustc_vv" | sed -n 's/^LLVM version: //p' | head -n1)
machine_rustc_vv_sha=$(printf '%s\n' "$machine_rustc_vv" | sha256_text)
machine_memory=$(awk '/^MemTotal:/ { print $2 * 1024; exit }' /proc/meminfo 2>/dev/null || true)
machine_governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo unknown)
compiler_sha256=$(sha256 "$ROOT/target/debug/jet")
machine_rustc_path=$(resolve_tool_path rustc || true)
machine_rustc_sha256=
if [ -n "$machine_rustc_path" ]; then
    machine_rustc_sha256=$(sha256 "$machine_rustc_path")
fi
machine_jet_env_sha256=$(sha256 "$JET_ENV")

machine_ldd_output=$(ldd "$JET_BIN" 2>&1 || true)
machine_libc_path=$(printf '%s\n' "$machine_ldd_output" \
    | sed -n 's/^[[:space:]]*libc[^ ]* => \([^ ]*\).*/\1/p' \
    | head -n1)
if [ -n "$machine_libc_path" ] && [ -f "$machine_libc_path" ]; then
    machine_libc_path=$(readlink -f "$machine_libc_path" 2>/dev/null || printf '%s' "$machine_libc_path")
    machine_libc_sha256=$(sha256 "$machine_libc_path")
    machine_libc_version=$(getconf GNU_LIBC_VERSION 2>/dev/null || \
        "$machine_libc_path" --version 2>/dev/null | sed -n '1p' || true)
else
    case "$machine_ldd_output" in
        *"statically linked"*|*"not a dynamic executable"*)
            machine_libc_path=static
            machine_libc_sha256=static
            machine_libc_version=static
            ;;
        *)
            echo "unavailable libc identity for $JET_BIN" >&2
            exit 1
            ;;
    esac
fi

machine_allocator='JetHostProgramAllocator->std::alloc::System'
machine_allocator_source_sha256=$(printf 'ProgramAllocator.rs=%s\nSource/main.rs=%s\n' \
    "$(sha256 "$ROOT/crates/jet-codegen/src/Prelude/ProgramAllocator.rs")" \
    "$(sha256 "$ROOT/Source/main.rs")" | sha256_text)
machine_allocator_environment=$(env | awk -F= '
    $1 == "LD_PRELOAD" || $1 == "GLIBC_TUNABLES" || $1 == "MALLOC_CONF" ||
    $1 ~ /^MALLOC_/ || $1 ~ /^JEMALLOC_/ || $1 ~ /^MIMALLOC_/ { print }
' | LC_ALL=C sort)
if [ -n "$machine_allocator_environment" ]; then
    echo "allocator override is active; compiler-speed evidence requires the hosted system allocator" >&2
    exit 1
fi
machine_allocator_environment_sha256=$(printf '%s\n' "$machine_allocator_environment" | sha256_text)

machine_lscpu_path=$(type -P lscpu 2>/dev/null || true)
machine_cpu_model=
machine_cpu_sockets=
machine_cpu_cores_per_socket=
machine_cpu_threads_per_core=
machine_cpu_numa_nodes=
machine_cpu_online=
machine_topology_sha256=
if [ -n "$machine_lscpu_path" ] && [ -x "$machine_lscpu_path" ]; then
    machine_cpu_model=$(LC_ALL=C "$machine_lscpu_path" | sed -n 's/^Model name:[[:space:]]*//p' | head -n1)
    machine_cpu_sockets=$(LC_ALL=C "$machine_lscpu_path" | sed -n 's/^Socket(s):[[:space:]]*//p' | head -n1)
    machine_cpu_cores_per_socket=$(LC_ALL=C "$machine_lscpu_path" | sed -n 's/^Core(s) per socket:[[:space:]]*//p' | head -n1)
    machine_cpu_threads_per_core=$(LC_ALL=C "$machine_lscpu_path" | sed -n 's/^Thread(s) per core:[[:space:]]*//p' | head -n1)
    machine_cpu_numa_nodes=$(LC_ALL=C "$machine_lscpu_path" | sed -n 's/^NUMA node(s):[[:space:]]*//p' | head -n1)
    machine_cpu_online=$(LC_ALL=C "$machine_lscpu_path" | sed -n 's/^On-line CPU(s) list:[[:space:]]*//p' | head -n1)
    machine_topology_sha256=$(LC_ALL=C "$machine_lscpu_path" -p=CPU,Core,Socket,Node 2>/dev/null \
        | sha256_text)
fi
machine_affinity=$(awk '/^Cpus_allowed_list:/ { print $2; exit }' /proc/self/status 2>/dev/null || true)
if [ -z "$machine_affinity" ]; then
    machine_affinity=$(taskset -pc $$ 2>/dev/null | sed 's/.*current affinity list: //' || true)
fi
machine_hardware_sha256=$(printf 'arch=%s\nmodel=%s\nsockets=%s\ncores_per_socket=%s\nthreads_per_core=%s\nnuma_nodes=%s\nonline=%s\ntopology=%s\naffinity=%s\ncpus=%s\nmemory=%s\nkernel=%s\ngovernor=%s\n' \
    "$machine_arch" "$machine_cpu_model" "$machine_cpu_sockets" "$machine_cpu_cores_per_socket" \
    "$machine_cpu_threads_per_core" "$machine_cpu_numa_nodes" "$machine_cpu_online" \
    "$machine_topology_sha256" "$machine_affinity" "$machine_cpus" "$machine_memory" \
    "$machine_kernel" "$machine_governor" | sha256_text)
machine_toolchain_sha256=$(printf 'jet=%s\njet_env=%s\nrustc=%s\nrustc_path=%s\nrustc_sha256=%s\nrustc_vv=%s\nllvm=%s\ntarget=%s\n' \
    "$compiler_sha256" "$machine_jet_env_sha256" "$machine_rustc" "$machine_rustc_path" \
    "$machine_rustc_sha256" "$machine_rustc_vv_sha" "$machine_llvm" "$machine_target" | sha256_text)

case "$machine_cpus" in
    ''|*[!0-9]*) echo "unavailable machine CPU identity: $machine_cpus" >&2; exit 1 ;;
esac
case "$machine_memory" in
    ''|*[!0-9]*) echo "unavailable machine memory identity" >&2; exit 1 ;;
esac
for identity_pair in \
    "machine host $machine_host" \
    "machine target $machine_target" \
    "machine rustc $machine_rustc" \
    "machine LLVM $machine_llvm" \
    "machine rustc-vV $machine_rustc_vv_sha" \
    "machine rustc path $machine_rustc_path" \
    "machine rustc digest $machine_rustc_sha256" \
    "machine kernel $machine_kernel" \
    "machine governor $machine_governor" \
    "machine libc version $machine_libc_version" \
    "machine libc path $machine_libc_path" \
    "machine libc digest $machine_libc_sha256" \
    "machine allocator digest $machine_allocator_source_sha256" \
    "machine allocator environment digest $machine_allocator_environment_sha256" \
    "machine CPU model $machine_cpu_model" \
    "machine CPU sockets $machine_cpu_sockets" \
    "machine CPU cores $machine_cpu_cores_per_socket" \
    "machine CPU threads $machine_cpu_threads_per_core" \
    "machine NUMA nodes $machine_cpu_numa_nodes" \
    "machine online CPUs $machine_cpu_online" \
    "machine topology digest $machine_topology_sha256" \
    "machine affinity $machine_affinity" \
    "machine hardware digest $machine_hardware_sha256" \
    "machine toolchain digest $machine_toolchain_sha256"; do
    identity_name=$(printf '%s %s' "$identity_pair" "identity" | awk '{print $1 " " $2}')
    identity_value=$(printf '%s\n' "$identity_pair" | cut -d' ' -f3-)
    require_identity "$identity_name" "$identity_value"
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
        check_corpus_file_size "$corpus_program"
        check_corpus_file_size "$corpus_expected"
        check_corpus_file_size "$corpus_edit"
        check_corpus_file_size "$corpus_edit_expected"
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
    for phase_name in parse sema ffi tir emission build_plan backend link frontend jit jit_cache_hit cache_hit rust_bytes; do
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
    "$TIME_BIN" -f '%e\t%M' -o "$timed_stats" \
        "$TIMEOUT_BIN" --signal=TERM --kill-after=5s "${TRIAL_DEADLINE_SECONDS}s" "$@" \
        >"$timed_stdout" 2>"$timed_stderr"
}

report_trial_failure() {
    trial_status=$1
    trial_work=$2
    trial_stderr=$3
    if [ "$trial_status" -eq 124 ] || [ "$trial_status" -eq 137 ]; then
        echo "pathological compiler workload exceeded ${TRIAL_DEADLINE_SECONDS}s: $trial_work/run.jet" >&2
    else
        echo "compiler workload failed: $trial_work/run.jet (exit $trial_status)" >&2
    fi
    sed -n '1,120p' "$trial_stderr" >&2 || true
    exit 1
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

set_linker_provenance() {
    linker_label=$1
    TRIAL_LINKER_PATH=none
    TRIAL_LINKER_SHA256=none
    TRIAL_LINKER_BACKEND=none
    TRIAL_LINKER_BACKEND_PATH=none
    TRIAL_LINKER_BACKEND_SHA256=none
    case "$linker_label" in
        *" via "*)
            linker_backend=${linker_label%% via *}
            linker_driver=${linker_label##* via }
            ;;
        explicit:*)
            linker_backend=explicit
            linker_driver=${linker_label#explicit:}
            ;;
        system)
            linker_backend=system
            linker_driver=$("$JET_ENV" rustc --print linker 2>/dev/null | sed -n '1p')
            ;;
        *)
            echo "unavailable linker identity: $linker_label" >&2
            exit 1
            ;;
    esac
    linker_driver_path=$(resolve_tool_path "$linker_driver" || true)
    [ -n "$linker_driver_path" ] || {
        echo "unavailable linker driver path: $linker_driver" >&2
        exit 1
    }
    TRIAL_LINKER_PATH=$linker_driver_path
    TRIAL_LINKER_SHA256=$(sha256 "$linker_driver_path")
    TRIAL_LINKER_BACKEND=$linker_backend
    if [ "$linker_backend" != "system" ] && [ "$linker_backend" != "explicit" ]; then
        linker_backend_path=$(resolve_tool_path "$linker_backend" || true)
        [ -n "$linker_backend_path" ] || {
            echo "unavailable linker backend path: $linker_backend" >&2
            exit 1
        }
        TRIAL_LINKER_BACKEND_PATH=$linker_backend_path
        TRIAL_LINKER_BACKEND_SHA256=$(sha256 "$linker_backend_path")
    fi
}

workload_digest() {
    printf 'jet.compiler-speed.workload.v1\nprogram=%s\nrole=%s\nsource_sha256=%s\nexpected_sha256=%s\nsource_bytes=%s\nexpected_bytes=%s\n' \
        "$1" "$2" "$3" "$4" "$5" "$6" | sha256_text
}

prepare_fixture() {
    fixture_work=$1
    fixture_program=$2
    fixture_expected=$3
    mkdir -p "$fixture_work"
    cp "$FIXTURE_PACKAGE" "$fixture_work/package.jet"
    cp "$ROOT/$fixture_program" "$fixture_work/run.jet"
    cp "$ROOT/$fixture_expected" "$fixture_work/expected.out"
    printf '%s\n' \
        'name: "compiler_speed_fixture"' \
        'version: "0.1.0"' \
        'authority: .{ holds: { allow: [IO, Mem.Alloc] } }' > "$fixture_work/package.jet"
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
    [ "$trial_status" -eq 0 ] || report_trial_failure "$trial_status" "$trial_work" "$trial_output.stderr"
    check_trial_output "$trial_work" "$trial_output.stdout" "$trial_output.stderr"
    trial_timing_file="$trial_work/timing/jet-timing.json"
    append_timing_phases "$trial_timing_file" "$trial_phases"
    trial_cache_hit=$(timing_field "$trial_timing_file" cache_hit)
    case "$trial_cache_hit" in
        1) ;;
        *) trial_cache_hit=0 ;;
    esac
    TRIAL_CACHE_HIT=$trial_cache_hit
    TRIAL_CACHE_MISSES=$((1 - trial_cache_hit))
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
    if [ "${JET_ENV_DISABLE:-0}" = "1" ]; then
        if run_timed "$trial_stats" "$trial_output.build.stdout" "$trial_output.build.stderr" \
            env \
            JET_CACHE_DIR="$trial_cache/build" \
            JET_ROOT="$trial_work" \
            JET_TIMING=1 \
            JET_TIMING_DIR="$trial_work/timing" \
            NO_COLOR=1 \
            sh -c "cd '$trial_work' && exec '$JET_BIN' build --profile=release --verbose run.jet"; then
            trial_status=0
        else
            trial_status=$?
        fi
    else
        if run_timed "$trial_stats" "$trial_output.build.stdout" "$trial_output.build.stderr" \
            env \
            JET_CACHE_DIR="$trial_cache/build" \
            JET_TIMING=1 \
            JET_TIMING_DIR="$trial_work/timing" \
            NO_COLOR=1 \
            "$JET_ENV" bash -c "cd '$trial_work' && exec jet build --profile=release --verbose run.jet"; then
            trial_status=0
        else
            trial_status=$?
        fi
    fi
    [ "$trial_status" -eq 0 ] || report_trial_failure "$trial_status" "$trial_work" "$trial_output.build.stderr"
    [ -x "$trial_work/build/run" ] || { echo "missing AOT artifact: $trial_work/build/run" >&2; exit 1; }
    if "$TIMEOUT_BIN" --signal=TERM --kill-after=5s "${TRIAL_DEADLINE_SECONDS}s" \
        "$trial_work/build/run" >"$trial_output.stdout" 2>"$trial_output.stderr"; then
        trial_status=0
    else
        trial_status=$?
    fi
    [ "$trial_status" -eq 0 ] || report_trial_failure "$trial_status" "$trial_work" "$trial_output.stderr"
    check_trial_output "$trial_work" "$trial_output.stdout" "$trial_output.stderr"
    append_timing_phases "$trial_work/timing/jet-timing.json" "$trial_phases"
    append_timing_phases "$trial_work/timing/build/jet-timing-backend.json" "$trial_phases"
    aot_cache_hits=$(grep -c '\[build\] cache hit -> reused cached binary' "$trial_output.build.stderr" || true)
    aot_cache_misses=$(grep -c '\[build\] cache miss -> compiling' "$trial_output.build.stderr" || true)
    TRIAL_CACHE_HITS=$aot_cache_hits
    TRIAL_CACHE_MISSES=$aot_cache_misses
    printf '%s%s%s\n' cache_hit "$TAB" "$aot_cache_hits" >> "$trial_phases"
    printf '%s%s%s\n' cache_miss "$TAB" "$aot_cache_misses" >> "$trial_phases"
    trial_linker=$(sed -n 's/.*\[build\] linker[[:space:]]*->[[:space:]]*//p' "$trial_output.build.stderr" | tail -n1)
    if [ -n "$trial_linker" ]; then
        if [ -n "${TRIAL_LINKER:-}" ] && [ "$TRIAL_LINKER" != "$trial_linker" ]; then
            echo "linker changed inside one compiler-speed state: $TRIAL_LINKER -> $trial_linker" >&2
            exit 1
        fi
        TRIAL_LINKER=$trial_linker
        set_linker_provenance "$trial_linker"
    fi
    TRIAL_LINKER=${TRIAL_LINKER:-unavailable}
    if [ "$TRIAL_LINKER" = "unavailable" ]; then
        echo "unavailable linker identity for $trial_work/run.jet" >&2
        exit 1
    fi
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

check_cache_state() {
    cache_program=$1
    cache_state=$2
    cache_kind=$3
    cache_stage=$4
    if [ "$cache_stage" = "jit-fast" ]; then
        cache_hits=$TRIAL_CACHE_HIT
    else
        cache_hits=$TRIAL_CACHE_HITS
    fi
    case "$cache_kind" in
        clean|edit)
            [ "$cache_hits" -eq 0 ] && [ "$TRIAL_CACHE_MISSES" -gt 0 ] || {
                echo "cache invalidation hidden for $cache_program/$cache_state: expected miss, hits=$cache_hits misses=$TRIAL_CACHE_MISSES" >&2
                exit 1
            }
            ;;
        no-change)
            [ "$cache_hits" -gt 0 ] && [ "$TRIAL_CACHE_MISSES" -eq 0 ] || {
                echo "no-change cache reuse missing for $cache_program/$cache_state: hits=$cache_hits misses=$TRIAL_CACHE_MISSES" >&2
                exit 1
            }
            ;;
        *)
            echo "unknown compiler-speed cache state: $cache_kind" >&2
            exit 1
            ;;
    esac
}

restore_edit_fixture() {
    edit_work=$1
    edit_cache=$2
    edit_warm_cache=$3
    edit_program=$4
    edit_expected=$5
    rm -rf "$edit_work/build" "$edit_work/cache" "$edit_work/timing" "$edit_work/jet-timing.json"
    cp -R "$edit_warm_cache" "$edit_cache"
    prepare_fixture "$edit_work" "$edit_program" "$edit_expected"
}

phase_average() {
    phase_file_path=$1
    phase_wanted=$2
    awk -F "$TAB" -v wanted="$phase_wanted" '$1 == wanted { total += $2; count++ } END { if (count == 0) print 0; else printf "%.0f\n", total / count }' "$phase_file_path"
}

required_phase_average() {
    phase_file_path=$1
    phase_wanted=$2
    phase_count=$(awk -F "$TAB" -v wanted="$phase_wanted" '$1 == wanted { count++ } END { print count + 0 }' "$phase_file_path")
    [ "$phase_count" -eq "$SAMPLES" ] || {
        echo "missing required compiler phase: $phase_wanted (expected $SAMPLES samples, got $phase_count)" >&2
        exit 1
    }
    phase_average "$phase_file_path" "$phase_wanted"
}

safe_id() {
    printf '%s' "$1" | tr '/.' '__'
}

parity_run_process() {
    parity_status_file=$1
    parity_stdout=$2
    parity_stderr=$3
    parity_cwd=$4
    shift 4
    if (cd "$parity_cwd" && "$TIMEOUT_BIN" --signal=TERM --kill-after=5s "${TRIAL_DEADLINE_SECONDS}s" "$@") >"$parity_stdout" 2>"$parity_stderr"; then
        parity_status=0
    else
        parity_status=$?
    fi
    printf '%s\n' "$parity_status" > "$parity_status_file"
}

parity_require_status() {
    parity_status=$1
    parity_label=$2
    [ "$parity_status" -eq 0 ] || {
        echo "parity command failed: $parity_label (exit $parity_status)" >&2
        exit 1
    }
}

parity_compare_status() {
    parity_left=$1
    parity_right=$2
    parity_label=$3
    [ "$parity_left" -eq "$parity_right" ] || {
        echo "parity exit mismatch: $parity_label ($parity_left -> $parity_right)" >&2
        exit 1
    }
}

parity_compare() {
    parity_left=$1
    parity_right=$2
    parity_label=$3
    cmp "$parity_left" "$parity_right" || {
        echo "parity mismatch: $parity_label" >&2
        diff -u "$parity_left" "$parity_right" >&2 || true
        exit 1
    }
}

parity_compare_snapshot_prefix() {
    parity_snapshot=$1
    parity_actual=$2
    parity_label=$3
    parity_snapshot_bytes=$(wc -c < "$parity_snapshot" | tr -d '[:space:]')
    parity_actual_bytes=$(wc -c < "$parity_actual" | tr -d '[:space:]')
    [ "$parity_actual_bytes" -ge "$parity_snapshot_bytes" ] || {
        echo "diagnostic became shorter: $parity_label" >&2
        exit 1
    }
    head -c "$parity_snapshot_bytes" "$parity_actual" | cmp - "$parity_snapshot" || {
        echo "diagnostic snapshot prefix mismatch: $parity_label" >&2
        diff -u "$parity_snapshot" "$parity_actual" >&2 || true
        exit 1
    }
}

parity_tier_mode() {
    parity_trace=$1
    parity_native=0
    parity_interpreter=0
    grep -Fq 'tier1 native' "$parity_trace" && parity_native=1 || true
    grep -Fq 'tier0 interp' "$parity_trace" && parity_interpreter=1 || true
    case "$parity_native:$parity_interpreter" in
        1:0) printf '%s\n' native ;;
        0:1) printf '%s\n' interpreter ;;
        1:1) printf '%s\n' mixed ;;
        *) echo "missing tier receipt: $parity_trace" >&2; exit 1 ;;
    esac
}

parity_require_tier() {
    parity_mode=$1
    parity_requirement=$2
    case "$parity_requirement:$parity_mode" in
        native:native|explicit:native|explicit:interpreter|explicit:mixed) ;;
        native:*) echo "speed result used non-native tier: $parity_mode" >&2; exit 1 ;;
        explicit:*) echo "missing explicit tier classification: $parity_mode" >&2; exit 1 ;;
        *) echo "unknown tier requirement: $parity_requirement" >&2; exit 1 ;;
    esac
}

parity_run_case() {
    parity_id=$1
    parity_program=$2
    parity_expected=$3
    parity_requirement=$4
    parity_root="$run_dir/parity-$parity_id"
    rm -rf "$parity_root"
    mkdir -p "$parity_root"
    prepare_fixture "$parity_root" "$parity_program" "$parity_expected"

    parity_run_process "$parity_root/jit.status" "$parity_root/jit.stdout" "$parity_root/jit.stderr" "$parity_root" \
        env JET_RUN_CACHE_DIR="$parity_root/jit-run-cache" JET_CACHE_DIR="$parity_root/jit-build-cache" NO_COLOR=1 \
        "$JET_BIN" run run.jet
    parity_run_process "$parity_root/dev.status" "$parity_root/dev.stdout" "$parity_root/dev.stderr" "$parity_root" \
        env JET_RUN_CACHE_DIR="$parity_root/dev-run-cache" JET_CACHE_DIR="$parity_root/dev-build-cache" NO_COLOR=1 \
        "$JET_BIN" dev run.jet --watch=off --quiet
    parity_run_process "$parity_root/aot-build.status" "$parity_root/aot-build.stdout" "$parity_root/aot-build.stderr" "$parity_root" \
        env JET_CACHE_DIR="$parity_root/aot-build-cache" NO_COLOR=1 \
        "$JET_ENV" bash -c "cd '$parity_root' && exec jet build --profile=release run.jet"
    parity_require_status "$(sed -n '1p' "$parity_root/jit.status")" "$parity_id/jit"
    parity_require_status "$(sed -n '1p' "$parity_root/dev.status")" "$parity_id/dev"
    parity_require_status "$(sed -n '1p' "$parity_root/aot-build.status")" "$parity_id/aot-build"
    [ -x "$parity_root/build/run" ] || { echo "missing parity AOT artifact: $parity_id" >&2; exit 1; }
    parity_run_process "$parity_root/aot.status" "$parity_root/aot.stdout" "$parity_root/aot.stderr" "$parity_root" \
        env NO_COLOR=1 "$parity_root/build/run"
    parity_require_status "$(sed -n '1p' "$parity_root/aot.status")" "$parity_id/aot"
    parity_compare_status "$(sed -n '1p' "$parity_root/jit.status")" "$(sed -n '1p' "$parity_root/dev.status")" "$parity_id/jit-dev"
    parity_compare_status "$(sed -n '1p' "$parity_root/jit.status")" "$(sed -n '1p' "$parity_root/aot.status")" "$parity_id/jit-aot"

    parity_compare "$parity_root/expected.out" "$parity_root/jit.stdout" "$parity_id/expected-jit-stdout"
    parity_compare "$parity_root/jit.stdout" "$parity_root/dev.stdout" "$parity_id/jit-dev-stdout"
    parity_compare "$parity_root/jit.stdout" "$parity_root/aot.stdout" "$parity_id/jit-aot-stdout"
    parity_compare "$parity_root/jit.stderr" "$parity_root/dev.stderr" "$parity_id/jit-dev-stderr"
    parity_compare "$parity_root/jit.stderr" "$parity_root/aot.stderr" "$parity_id/jit-aot-stderr"

    parity_run_process "$parity_root/jit-trace.status" "$parity_root/jit-trace.stdout" "$parity_root/jit-trace.stderr" "$parity_root" \
        env JET_RUN_CACHE_DIR="$parity_root/jit-trace-run-cache" JET_CACHE_DIR="$parity_root/jit-trace-build-cache" NO_COLOR=1 \
        "$JET_BIN" run run.jet --trace-tiers
    parity_run_process "$parity_root/dev-trace.status" "$parity_root/dev-trace.stdout" "$parity_root/dev-trace.stderr" "$parity_root" \
        env JET_RUN_CACHE_DIR="$parity_root/dev-trace-run-cache" JET_CACHE_DIR="$parity_root/dev-trace-build-cache" NO_COLOR=1 \
        "$JET_BIN" dev run.jet --watch=off --quiet --trace-tiers
    parity_require_status "$(sed -n '1p' "$parity_root/jit-trace.status")" "$parity_id/jit-trace"
    parity_require_status "$(sed -n '1p' "$parity_root/dev-trace.status")" "$parity_id/dev-trace"
    parity_jit_tier=$(parity_tier_mode "$parity_root/jit-trace.stderr")
    parity_dev_tier=$(parity_tier_mode "$parity_root/dev-trace.stderr")
    [ "$parity_jit_tier" = "$parity_dev_tier" ] || {
        echo "JIT/dev tier divergence: $parity_id ($parity_jit_tier -> $parity_dev_tier)" >&2
        exit 1
    }
    parity_require_tier "$parity_jit_tier" "$parity_requirement"
    PARITY_CASE_COUNT=$((PARITY_CASE_COUNT + 1))
}

parity_run_dev_case() {
    parity_id=$1
    parity_program=$2
    parity_expected=$3
    parity_jit_state=$4
    parity_root="$run_dir/parity-dev-$parity_id"
    parity_jit_id=$(safe_id "$parity_jit_state")
    parity_jit_stdout="$outputs_dir/$parity_jit_id.stdout"
    parity_jit_stderr="$outputs_dir/$parity_jit_id.stderr"
    [ -f "$parity_jit_stdout" ] && [ -f "$parity_jit_stderr" ] || {
        echo "missing measured JIT receipt for parity: $parity_jit_state" >&2
        exit 1
    }
    rm -rf "$parity_root"
    mkdir -p "$parity_root"
    prepare_fixture "$parity_root" "$parity_program" "$parity_expected"
    parity_run_process "$parity_root/dev.status" "$parity_root/dev.stdout" "$parity_root/dev.stderr" "$parity_root" \
        env JET_RUN_CACHE_DIR="$parity_root/dev-run-cache" JET_CACHE_DIR="$parity_root/dev-build-cache" NO_COLOR=1 \
        "$JET_BIN" dev run.jet --watch=off --quiet
    parity_require_status "$(sed -n '1p' "$parity_root/dev.status")" "$parity_id/dev"
    parity_compare "$parity_root/expected.out" "$parity_root/dev.stdout" "$parity_id/expected-dev-stdout"
    parity_compare "$parity_jit_stdout" "$parity_root/dev.stdout" "$parity_id/jit-dev-stdout"
    parity_compare "$parity_jit_stderr" "$parity_root/dev.stderr" "$parity_id/jit-dev-stderr"

    parity_run_process "$parity_root/jit-trace.status" "$parity_root/jit-trace.stdout" "$parity_root/jit-trace.stderr" "$parity_root" \
        env JET_RUN_CACHE_DIR="$parity_root/jit-trace-run-cache" JET_CACHE_DIR="$parity_root/jit-trace-build-cache" NO_COLOR=1 \
        "$JET_BIN" run run.jet --trace-tiers
    parity_run_process "$parity_root/dev-trace.status" "$parity_root/dev-trace.stdout" "$parity_root/dev-trace.stderr" "$parity_root" \
        env JET_RUN_CACHE_DIR="$parity_root/dev-trace-run-cache" JET_CACHE_DIR="$parity_root/dev-trace-build-cache" NO_COLOR=1 \
        "$JET_BIN" dev run.jet --watch=off --quiet --trace-tiers
    parity_require_status "$(sed -n '1p' "$parity_root/jit-trace.status")" "$parity_id/jit-trace"
    parity_require_status "$(sed -n '1p' "$parity_root/dev-trace.status")" "$parity_id/dev-trace"
    parity_jit_tier=$(parity_tier_mode "$parity_root/jit-trace.stderr")
    parity_dev_tier=$(parity_tier_mode "$parity_root/dev-trace.stderr")
    [ "$parity_jit_tier" = "$parity_dev_tier" ] || {
        echo "JIT/dev tier divergence: $parity_id ($parity_jit_tier -> $parity_dev_tier)" >&2
        exit 1
    }
    parity_require_tier "$parity_jit_tier" native
    PARITY_CASE_COUNT=$((PARITY_CASE_COUNT + 1))
}

parity_check_diagnostic() {
    parity_id=diagnostic-arg-type-mismatch
    parity_root="$run_dir/parity-$parity_id"
    parity_source=tests/ui/arg_type_mismatch.jet
    parity_snapshot=tests/ui/arg_type_mismatch.stderr
    rm -rf "$parity_root"
    mkdir -p "$parity_root"
    parity_run_process "$parity_root/jit.status" "$parity_root/jit.stdout" "$parity_root/jit.stderr" "$ROOT" \
        env JET_RUN_CACHE_DIR="$parity_root/jit-run-cache" JET_CACHE_DIR="$parity_root/jit-build-cache" NO_COLOR=1 \
        "$JET_BIN" run "$parity_source"
    parity_run_process "$parity_root/dev.status" "$parity_root/dev.stdout" "$parity_root/dev.stderr" "$ROOT" \
        env JET_RUN_CACHE_DIR="$parity_root/dev-run-cache" JET_CACHE_DIR="$parity_root/dev-build-cache" NO_COLOR=1 \
        "$JET_BIN" dev "$parity_source" --watch=off --quiet
    parity_run_process "$parity_root/aot.status" "$parity_root/aot.stdout" "$parity_root/aot.stderr" "$ROOT" \
        env JET_CACHE_DIR="$parity_root/aot-build-cache" NO_COLOR=1 \
        "$JET_ENV" bash -c "cd '$ROOT' && exec jet build --profile=release '$parity_source'"
    for parity_tier in jit dev aot; do
        parity_status=$(sed -n '1p' "$parity_root/$parity_tier.status")
        [ "$parity_status" -ne 0 ] || { echo "diagnostic unexpectedly passed: $parity_tier" >&2; exit 1; }
        [ ! -s "$parity_root/$parity_tier.stdout" ] || { echo "diagnostic leaked to stdout: $parity_tier" >&2; exit 1; }
        parity_compare_snapshot_prefix "$parity_snapshot" "$parity_root/$parity_tier.stderr" "$parity_id/$parity_tier"
        for parity_field in 'Error [E0112]' ' Why:' ' Fix:'; do
            grep -Fq "$parity_field" "$parity_root/$parity_tier.stderr" || {
                echo "diagnostic field missing: $parity_tier/$parity_field" >&2
                exit 1
            }
        done
    done
    parity_compare_status "$(sed -n '1p' "$parity_root/jit.status")" "$(sed -n '1p' "$parity_root/dev.status")" "$parity_id/jit-dev"
    parity_compare_status "$(sed -n '1p' "$parity_root/jit.status")" "$(sed -n '1p' "$parity_root/aot.status")" "$parity_id/jit-aot"
    parity_compare "$parity_root/jit.stderr" "$parity_root/dev.stderr" "$parity_id/jit-dev-stderr"
    parity_compare "$parity_root/jit.stderr" "$parity_root/aot.stderr" "$parity_id/jit-aot-stderr"
    PARITY_CASE_COUNT=$((PARITY_CASE_COUNT + 1))
}

run_parity_checks() {
    while IFS="$TAB" read -r parity_program parity_expected parity_source_hash parity_expected_hash parity_edit parity_edit_expected parity_edit_hash parity_edit_expected_hash; do
        case "$parity_program" in
            ""|\#*) continue ;;
        esac
        parity_run_dev_case "corpus-$(safe_id "$parity_program")" "$parity_program" "$parity_expected" "$parity_program-jit-clean"
        parity_run_dev_case "edit-$(safe_id "$parity_edit")" "$parity_edit" "$parity_edit_expected" "$parity_edit-jit-representative-edit"
    done < "$CORPUS"
    parity_run_case trait-object examples/features/types/traits.jet examples/features/expected/types/traits.out explicit
    parity_run_case effects examples/features/effects/effects.jet examples/features/expected/effects/effects.out native
    parity_run_case closure examples/features/basics/bare_lambda_param.jet examples/features/expected/basics/bare_lambda_param.out native
    parity_check_diagnostic
    PARITY_RECEIPT=verified
    parity_rows_file="$run_dir/rows.with-parity.tsv"
    : > "$parity_rows_file"
    while IFS="$TAB" read -r parity_program parity_state parity_stage parity_latency parity_memory parity_variance parity_stdout parity_stderr parity_phases; do
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$parity_program" "$parity_state" "$parity_stage" "$parity_latency" "$parity_memory" "$parity_variance" \
            "$parity_stdout" "$parity_stderr" "$parity_phases;parity=$PARITY_RECEIPT;semantic_parity=$PARITY_RECEIPT;diagnostic_parity=$PARITY_RECEIPT;effect_parity=$PARITY_RECEIPT;tier_parity=$PARITY_RECEIPT;dev_profile=dev;aot_profile=release" >> "$parity_rows_file"
    done < "$rows_file"
    mv "$parity_rows_file" "$rows_file"
}

row_field() {
    row_field_program=$1
    row_field_state=$2
    row_field_number=$3
    awk -F "$TAB" -v program="$row_field_program" -v state="$row_field_state" -v field="$row_field_number" '$1 == program && $2 == state { print $field; exit }' "$rows_file"
}

row_phase_value() {
    row_phase_program=$1
    row_phase_state=$2
    row_phase_name=$3
    row_phase_text=$(row_field "$row_phase_program" "$row_phase_state" 9)
    printf '%s\n' "$row_phase_text" | sed -n "s/.*;$row_phase_name=\\([^;]*\\).*/\\1/p"
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
    TRIAL_LINKER=
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
            state_warm_cache="$state_root/warm-cache"
            rm -rf "$state_warm_cache"
            cp -R "$state_cache" "$state_warm_cache"
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
        elif [ "$state_kind" = "edit" ]; then
            restore_edit_fixture "$state_work" "$state_cache" "$state_warm_cache" "$state_edit_program" "$state_edit_expected"
            sample_work="$state_work"
            sample_cache="$state_cache"
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
        check_cache_state "$state_program" "$state_name" "$state_kind" "$state_stage"
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
    if [ "$state_variance" -gt "$VARIANCE_BUDGET_PCT" ]; then
        echo "unstable compiler-speed benchmark: $state_program/$state_name variance=${state_variance}% budget=${VARIANCE_BUDGET_PCT}%" >&2
        exit 1
    fi
    if [ "$state_stage" = "jit-fast" ]; then
        state_backend="cranelift"
        state_linker="none"
        state_linker_path="none"
        state_linker_sha256="none"
        state_linker_backend="none"
        state_linker_backend_path="none"
        state_linker_backend_sha256="none"
        state_profile="fast"
        state_artifact_bytes=0
        state_phase_text="frontend_us=$(phase_average "$state_phases" frontend);jit_us=$(phase_average "$state_phases" jit);jit_cache_hit=$(phase_average "$state_phases" jit_cache_hit)"
    else
        state_backend="rustc-llvm"
        state_linker="$TRIAL_LINKER"
        state_linker_path="$TRIAL_LINKER_PATH"
        state_linker_sha256="$TRIAL_LINKER_SHA256"
        state_linker_backend="$TRIAL_LINKER_BACKEND"
        state_linker_backend_path="$TRIAL_LINKER_BACKEND_PATH"
        state_linker_backend_sha256="$TRIAL_LINKER_BACKEND_SHA256"
        state_profile="release"
        state_artifact_bytes="$TRIAL_ARTIFACT_BYTES"
        state_phase_text="parse_us=$(required_phase_average "$state_phases" parse);sema_us=$(required_phase_average "$state_phases" sema);ffi_us=$(phase_average "$state_phases" ffi);tir_us=$(required_phase_average "$state_phases" tir);emission_us=$(required_phase_average "$state_phases" emission);build_plan_us=$(phase_average "$state_phases" build_plan);backend_us=$(required_phase_average "$state_phases" backend);link_us=$(required_phase_average "$state_phases" link)"
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
    state_source_bytes=$(file_bytes "$ROOT/$state_source")
    state_expected_bytes=$(file_bytes "$ROOT/$state_expected_source")
    state_workload_sha256=$(workload_digest "$state_program" "$state_role" "$state_source_hash" "$state_expected_hash" "$state_source_bytes" "$state_expected_bytes")
    case "$state_kind" in
        clean) state_cache_state=Clean; state_cache_policy=fresh-cache-per-sample ;;
        no-change) state_cache_state=NoChange; state_cache_policy=shared-cache-after-warmup ;;
        edit) state_cache_state=Edit; state_cache_policy=base-cache-snapshot-before-edit ;;
        *) echo "unknown compiler-speed cache state: $state_kind" >&2; exit 1 ;;
    esac
    state_phase_text="$state_phase_text;source=$state_source;source_sha256=$state_source_hash;source_bytes=$state_source_bytes;expected_sha256=$state_expected_hash;expected_bytes=$state_expected_bytes;workload_sha256=$state_workload_sha256;role=$state_role;profile=$state_profile;backend=$state_backend;linker=$state_linker;linker_path=$state_linker_path;linker_sha256=$state_linker_sha256;linker_backend=$state_linker_backend;linker_backend_path=$state_linker_backend_path;linker_backend_sha256=$state_linker_backend_sha256;cache_state=$state_cache_state;cache_policy=$state_cache_policy;cache_hits=$(phase_average "$state_phases" cache_hit);cache_misses=$(phase_average "$state_phases" cache_miss);generated_rust_bytes=$(phase_average "$state_phases" rust_bytes);artifact_bytes=$state_artifact_bytes;libc_sha256=$machine_libc_sha256;allocator_sha256=$machine_allocator_source_sha256;allocator_environment_sha256=$machine_allocator_environment_sha256;hardware_sha256=$machine_hardware_sha256;topology_sha256=$machine_topology_sha256;toolchain_sha256=$machine_toolchain_sha256;rustc_sha256=$machine_rustc_sha256;top_cause=$(top_cause "$state_phases");full_artifact=$state_full_artifact"
    printf '%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s\n' \
        "$state_program" "$TAB" "$state_name" "$TAB" "$state_stage" "$TAB" \
        "$state_latency_median" "$TAB" "$state_memory_max" "$TAB" "$state_variance" "$TAB" \
        "$(sha256 "$state_reference_stdout")" "$TAB" "$(sha256 "$state_reference_stderr")" "$TAB" \
        "$state_phase_text" >> "$rows_file"
}

print_environment_json() {
    printf '{"schema":"jet.compiler-speed.environment","version":1,"report_version":%s,"corpus_sha256":%s,"corpus_count":%s,' \
        "$REPORT_VERSION" "$(json_q "$corpus_sha")" "$corpus_count"
    printf '"machine":{"allocator":%s,"allocator_environment_sha256":%s,"allocator_source_sha256":%s,"arch":%s,"compiler_sha256":%s,"cpus":%s,"cpu_model":%s,"cpu_numa_nodes":%s,"cpu_online":%s,"cpu_cores_per_socket":%s,"cpu_sockets":%s,"cpu_threads_per_core":%s,"governor":%s,"hardware_sha256":%s,"hostname":%s,"kernel":%s,"libc_path":%s,"libc_sha256":%s,"libc_version":%s,"memory_bytes":%s,"os":%s,"rustc":%s,"rustc_path":%s,"rustc_sha256":%s,"rustc_vv_sha256":%s,"target":%s,"toolchain_sha256":%s,"topology_sha256":%s,"affinity":%s,"jet_env_sha256":%s},' \
        "$(json_q "$machine_allocator")" "$(json_q "$machine_allocator_environment_sha256")" "$(json_q "$machine_allocator_source_sha256")" \
        "$(json_q "$machine_arch")" "$(json_q "$compiler_sha256")" "$machine_cpus" "$(json_q "$machine_cpu_model")" \
        "$machine_cpu_numa_nodes" "$(json_q "$machine_cpu_online")" "$machine_cpu_cores_per_socket" "$machine_cpu_sockets" \
        "$machine_cpu_threads_per_core" "$(json_q "$machine_governor")" "$(json_q "$machine_hardware_sha256")" \
        "$(json_q "$machine_host")" "$(json_q "$machine_kernel")" "$(json_q "$machine_libc_path")" \
        "$(json_q "$machine_libc_sha256")" "$(json_q "$machine_libc_version")" "$machine_memory" "$(json_q "$machine_os")" \
        "$(json_q "$machine_rustc")" "$(json_q "$machine_rustc_path")" "$(json_q "$machine_rustc_sha256")" \
        "$(json_q "$machine_rustc_vv_sha")" "$(json_q "$machine_target")" "$(json_q "$machine_toolchain_sha256")" \
        "$(json_q "$machine_topology_sha256")" "$(json_q "$machine_affinity")" "$(json_q "$machine_jet_env_sha256")"
    printf '"contract":{"cache_states":["Clean","NoChange","Edit"],"identity":"program+role+source_sha256+expected_sha256+source_bytes+expected_bytes","comparison":"same workload identity only","unmatched_workloads":"reject","profiles":["fast","release"],"backends":["cranelift","rustc-llvm"]}}\n'
}

check_construct_scale() {
    [ -f "$SCALE_CORPUS" ] || { echo "missing construct-scale matrix: $SCALE_CORPUS" >&2; exit 1; }
    scale_count=0
    scale_inputs="$run_dir/construct-scale-inputs.tsv"
    : > "$scale_inputs"
    while IFS="$TAB" read -r scale_construct scale_point scale_axis scale_instantiations scale_glue_units scale_source scale_expected; do
        case "$scale_construct" in
            ""|\#*) continue ;;
            generic-instantiations|bounded-variadics|derives-reflection|large-matches|closures|taskgroups-select|drop-cleanup) ;;
            *) echo "unknown construct-scale family: $scale_construct" >&2; exit 1 ;;
        esac
        case "$scale_point" in
            1|2|4) ;;
            *) echo "invalid construct-scale scale: $scale_construct/$scale_point" >&2; exit 1 ;;
        esac
        for scale_number in "$scale_instantiations" "$scale_glue_units"; do
            case "$scale_number" in
                ''|*[!0-9]*) echo "invalid construct-scale metadata: $scale_construct/$scale_point" >&2; exit 1 ;;
            esac
        done
        case "$scale_axis" in
            ""|*' '*) echo "invalid construct-scale axis: $scale_construct/$scale_point" >&2; exit 1 ;;
        esac
        require_relative_file "$scale_source"
        require_relative_file "$scale_expected"
        check_corpus_file_size "$scale_source"
        check_corpus_file_size "$scale_expected"
        scale_source_sha256=$(sha256 "$ROOT/$scale_source")
        scale_expected_sha256=$(sha256 "$ROOT/$scale_expected")
        printf '%s\t%s\t%s\t%s\n' "$scale_construct" "$scale_point" "$scale_source_sha256" "$scale_expected_sha256" >> "$scale_inputs"
        scale_count=$((scale_count + 1))
    done < "$SCALE_CORPUS"
    [ "$scale_count" -eq 21 ] || {
        echo "construct-scale matrix must contain 21 rows, got $scale_count" >&2
        exit 1
    }
    for scale_family in generic-instantiations bounded-variadics derives-reflection large-matches closures taskgroups-select drop-cleanup; do
        scale_family_count=$(awk -F "$TAB" -v family="$scale_family" '$1 == family { count++ } END { print count + 0 }' "$SCALE_CORPUS")
        [ "$scale_family_count" -eq 3 ] || {
            echo "construct-scale family must contain three points: $scale_family ($scale_family_count)" >&2
            exit 1
        }
    done
    SCALE_CORPUS_COUNT=$scale_count
    SCALE_CORPUS_SHA256=$(sha256 "$SCALE_CORPUS")
    SCALE_INPUTS_SHA256=$(sha256 "$scale_inputs")
}

percent_growth() {
    awk -v previous="$1" -v current="$2" 'BEGIN {
        if (previous == 0) { print 0; exit }
        printf "%.0f\n", ((current - previous) * 100) / previous
    }'
}

superlinear_growth() {
    awk -v previous_axis="$1" -v current_axis="$2" -v previous="$3" -v current="$4" 'BEGIN {
        if (current * previous_axis > previous * current_axis) print "yes"
        else print "no"
    }'
}

measure_construct_scale_state() {
    scale_state_construct=$1
    scale_state_point=$2
    scale_state_source=$3
    scale_state_expected=$4
    scale_state_id=$(safe_id "$scale_state_construct-$scale_state_point")
    scale_state_root="$run_dir/construct-$scale_state_id"
    scale_state_jit_phases="$scale_state_root/jit-phases.tsv"
    scale_state_aot_phases="$scale_state_root/aot-phases.tsv"
    scale_state_jit_latency="$scale_state_root/jit-latency.ns"
    scale_state_aot_latency="$scale_state_root/aot-latency.ns"
    scale_state_jit_memory="$scale_state_root/jit-memory.bytes"
    scale_state_aot_memory="$scale_state_root/aot-memory.bytes"
    mkdir -p "$scale_state_root"
    : > "$scale_state_jit_phases"
    : > "$scale_state_aot_phases"
    : > "$scale_state_jit_latency"
    : > "$scale_state_aot_latency"
    : > "$scale_state_jit_memory"
    : > "$scale_state_aot_memory"

    scale_state_jit_warmup="$scale_state_root/jit-warmup"
    prepare_fixture "$scale_state_jit_warmup" "$scale_state_source" "$scale_state_expected"
    run_jit_trial "$scale_state_jit_warmup" "$scale_state_jit_warmup/cache" "$scale_state_root/jit-warmup" "$scale_state_root/jit-warmup.stats" "$scale_state_jit_phases"
    scale_state_aot_warmup="$scale_state_root/aot-warmup"
    prepare_fixture "$scale_state_aot_warmup" "$scale_state_source" "$scale_state_expected"
    TRIAL_LINKER=
    run_aot_trial "$scale_state_aot_warmup" "$scale_state_aot_warmup/cache" "$scale_state_root/aot-warmup" "$scale_state_root/aot-warmup.stats" "$scale_state_aot_phases"
    : > "$scale_state_jit_phases"
    : > "$scale_state_aot_phases"

    scale_state_index=1
    while [ "$scale_state_index" -le "$SAMPLES" ]; do
        scale_state_jit_work="$scale_state_root/jit-$scale_state_index"
        prepare_fixture "$scale_state_jit_work" "$scale_state_source" "$scale_state_expected"
        run_jit_trial "$scale_state_jit_work" "$scale_state_jit_work/cache" "$scale_state_root/jit-$scale_state_index" "$scale_state_root/jit-$scale_state_index.stats" "$scale_state_jit_phases"
        printf '%s\n' "$TRIAL_LATENCY_NS" >> "$scale_state_jit_latency"
        printf '%s\n' "$TRIAL_MEMORY_BYTES" >> "$scale_state_jit_memory"

        scale_state_aot_work="$scale_state_root/aot-$scale_state_index"
        prepare_fixture "$scale_state_aot_work" "$scale_state_source" "$scale_state_expected"
        run_aot_trial "$scale_state_aot_work" "$scale_state_aot_work/cache" "$scale_state_root/aot-$scale_state_index" "$scale_state_root/aot-$scale_state_index.stats" "$scale_state_aot_phases"
        printf '%s\n' "$TRIAL_LATENCY_NS" >> "$scale_state_aot_latency"
        printf '%s\n' "$TRIAL_MEMORY_BYTES" >> "$scale_state_aot_memory"
        scale_state_aot_artifact_bytes=$TRIAL_ARTIFACT_BYTES
        scale_state_index=$((scale_state_index + 1))
    done

    cmp "$scale_state_root/jit-1.stdout" "$scale_state_root/aot-1.stdout" || {
        echo "JIT/AOT stdout parity failed: $scale_state_construct/$scale_state_point" >&2
        exit 1
    }
    cmp "$scale_state_root/jit-1.stderr" "$scale_state_root/aot-1.stderr" || {
        echo "JIT/AOT stderr parity failed: $scale_state_construct/$scale_state_point" >&2
        exit 1
    }
    SCALE_STATE_SOURCE_SHA256=$(sha256 "$ROOT/$scale_state_source")
    SCALE_STATE_EXPECTED_SHA256=$(sha256 "$ROOT/$scale_state_expected")
    SCALE_STATE_JIT_LATENCY=$(median_file "$scale_state_jit_latency")
    SCALE_STATE_AOT_LATENCY=$(median_file "$scale_state_aot_latency")
    SCALE_STATE_JIT_MEMORY=$(sort -n "$scale_state_jit_memory" | tail -n1)
    SCALE_STATE_AOT_MEMORY=$(sort -n "$scale_state_aot_memory" | tail -n1)
    SCALE_STATE_JIT_VARIANCE=$(variance_file "$scale_state_jit_latency")
    SCALE_STATE_AOT_VARIANCE=$(variance_file "$scale_state_aot_latency")
    [ "$SCALE_STATE_JIT_VARIANCE" -le "$VARIANCE_BUDGET_PCT" ] || {
        echo "unstable construct-scale JIT row: $scale_state_construct/$scale_state_point" >&2
        exit 1
    }
    [ "$SCALE_STATE_AOT_VARIANCE" -le "$VARIANCE_BUDGET_PCT" ] || {
        echo "unstable construct-scale AOT row: $scale_state_construct/$scale_state_point" >&2
        exit 1
    }
    SCALE_STATE_GENERATED_RUST_BYTES=$(phase_average "$scale_state_aot_phases" rust_bytes)
    [ "$SCALE_STATE_GENERATED_RUST_BYTES" -gt 0 ] || {
        echo "missing generated-Rust measurement: $scale_state_construct/$scale_state_point" >&2
        exit 1
    }
    SCALE_STATE_GLUE_BYTES=$SCALE_STATE_GENERATED_RUST_BYTES
    SCALE_STATE_ARTIFACT_BYTES=$scale_state_aot_artifact_bytes
}

print_construct_scale() {
    printf 'compiler-construct-scale version=1 matrix=%s matrix_sha256=%s inputs_sha256=%s stage=construct-scale machine=%s target=%s rustc=%s llvm=%s warmups=%s samples=%s\n' \
        "$SCALE_CORPUS_COUNT" "$SCALE_CORPUS_SHA256" "$SCALE_INPUTS_SHA256" "$machine" "$machine_target" "$machine_rustc" "$machine_llvm" "$WARMUPS" "$SAMPLES"
    printf '%s\n' 'construct scale axis instantiations glue_units jit_latency_ns aot_latency_ns generated_rust_bytes glue_bytes artifact_size_bytes jit_memory_bytes aot_memory_bytes jit_variance_pct aot_variance_pct aot_latency_growth_pct artifact_growth_pct superlinear_growth source source_sha256 expected expected_sha256'
    while IFS="$TAB" read -r scale_construct scale_point scale_axis scale_instantiations scale_glue_units scale_jit_latency scale_aot_latency scale_generated_rust scale_glue_bytes scale_artifact scale_jit_memory scale_aot_memory scale_jit_variance scale_aot_variance scale_aot_growth scale_artifact_growth scale_superlinear scale_source scale_source_sha256 scale_expected scale_expected_sha256; do
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$scale_construct" "$scale_point" "$scale_axis" "$scale_instantiations" "$scale_glue_units" "$scale_jit_latency" "$scale_aot_latency" "$scale_generated_rust" "$scale_glue_bytes" "$scale_artifact" "$scale_jit_memory" "$scale_aot_memory" "$scale_jit_variance" "$scale_aot_variance" "$scale_aot_growth" "$scale_artifact_growth" "$scale_superlinear" "$scale_source" "$scale_source_sha256" "$scale_expected" "$scale_expected_sha256"
    done < "$scale_rows_file"
}

print_construct_scale_json() {
    printf '{"schema":"jet.compiler-speed.construct-scale","version":1,"matrix_sha256":%s,"inputs_sha256":%s,"machine":%s,"target":%s,"rustc":%s,"llvm":%s,"warmups":%s,"samples":%s,"runs":[' \
        "$(json_q "$SCALE_CORPUS_SHA256")" "$(json_q "$SCALE_INPUTS_SHA256")" "$(json_q "$machine")" "$(json_q "$machine_target")" "$(json_q "$machine_rustc")" "$(json_q "$machine_llvm")" "$WARMUPS" "$SAMPLES"
    scale_json_first=1
    while IFS="$TAB" read -r scale_construct scale_point scale_axis scale_instantiations scale_glue_units scale_jit_latency scale_aot_latency scale_generated_rust scale_glue_bytes scale_artifact scale_jit_memory scale_aot_memory scale_jit_variance scale_aot_variance scale_aot_growth scale_artifact_growth scale_superlinear scale_source scale_source_sha256 scale_expected scale_expected_sha256; do
        [ "$scale_json_first" -eq 1 ] || printf ','
        scale_json_first=0
        printf '{"construct":%s,"scale":%s,"axis":%s,"instantiations":%s,"glue_units":%s,"jit_latency_ns":%s,"aot_latency_ns":%s,"generated_rust_bytes":%s,"glue_bytes":%s,"artifact_size_bytes":%s,"jit_memory_bytes":%s,"aot_memory_bytes":%s,"jit_variance_pct":%s,"aot_variance_pct":%s,"aot_latency_growth_pct":%s,"artifact_growth_pct":%s,"superlinear_growth":%s,"source":%s,"source_sha256":%s,"expected":%s,"expected_sha256":%s}' \
            "$(json_q "$scale_construct")" "$scale_point" "$(json_q "$scale_axis")" "$scale_instantiations" "$scale_glue_units" "$scale_jit_latency" "$scale_aot_latency" "$scale_generated_rust" "$scale_glue_bytes" "$scale_artifact" "$scale_jit_memory" "$scale_aot_memory" "$scale_jit_variance" "$scale_aot_variance" "$scale_aot_growth" "$scale_artifact_growth" "$(json_q "$scale_superlinear")" "$(json_q "$scale_source")" "$(json_q "$scale_source_sha256")" "$(json_q "$scale_expected")" "$(json_q "$scale_expected_sha256")"
    done < "$scale_rows_file"
    printf ']}\n'
}

if [ "${1:-}" = "--construct-scale" ] || [ "${1:-}" = "--construct-scale-json" ]; then
    case "$SCALE_SAMPLES" in
        ''|*[!0-9]*|0) echo "JET_PERF_SCALE_SAMPLES must be a positive integer" >&2; exit 2 ;;
    esac
    check_construct_scale
    SAMPLES=$SCALE_SAMPLES
    scale_rows_file="$run_dir/construct-scale-rows.tsv"
    : > "$scale_rows_file"
    scale_previous_construct=
    scale_previous_axis=
    scale_previous_aot_latency=
    scale_previous_artifact=
    while IFS="$TAB" read -r scale_construct scale_point scale_axis scale_instantiations scale_glue_units scale_source scale_expected; do
        case "$scale_construct" in
            ""|\#*) continue ;;
        esac
        measure_construct_scale_state "$scale_construct" "$scale_point" "$scale_source" "$scale_expected"
        if [ "$scale_construct" != "$scale_previous_construct" ]; then
            scale_aot_growth=0
            scale_artifact_growth=0
            scale_superlinear=baseline
        else
            scale_aot_growth=$(percent_growth "$scale_previous_aot_latency" "$SCALE_STATE_AOT_LATENCY")
            scale_artifact_growth=$(percent_growth "$scale_previous_artifact" "$SCALE_STATE_ARTIFACT_BYTES")
            scale_superlinear=$(superlinear_growth "$scale_previous_axis" "$scale_point" "$scale_previous_aot_latency" "$SCALE_STATE_AOT_LATENCY")
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$scale_construct" "$scale_point" "$scale_axis" "$scale_instantiations" "$scale_glue_units" "$SCALE_STATE_JIT_LATENCY" "$SCALE_STATE_AOT_LATENCY" "$SCALE_STATE_GENERATED_RUST_BYTES" "$SCALE_STATE_GLUE_BYTES" "$SCALE_STATE_ARTIFACT_BYTES" "$SCALE_STATE_JIT_MEMORY" "$SCALE_STATE_AOT_MEMORY" "$SCALE_STATE_JIT_VARIANCE" "$SCALE_STATE_AOT_VARIANCE" "$scale_aot_growth" "$scale_artifact_growth" "$scale_superlinear" "$scale_source" "$SCALE_STATE_SOURCE_SHA256" "$scale_expected" "$SCALE_STATE_EXPECTED_SHA256" >> "$scale_rows_file"
        scale_previous_construct=$scale_construct
        scale_previous_axis=$scale_point
        scale_previous_aot_latency=$SCALE_STATE_AOT_LATENCY
        scale_previous_artifact=$SCALE_STATE_ARTIFACT_BYTES
    done < "$SCALE_CORPUS"
    case "${1:-}" in
        --construct-scale) print_construct_scale ;;
        --construct-scale-json) print_construct_scale_json ;;
    esac
    exit 0
fi

corpus_count=$(check_corpus)
if [ "${1:-}" = "--environment" ]; then
    print_environment_json
    exit 0
fi
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
        for workload_field in workload_sha256 source_sha256 source_bytes expected_sha256 expected_bytes; do
            jit_workload_value=$(row_phase_value "$program" "$jit_state" "$workload_field")
            aot_workload_value=$(row_phase_value "$program" "$aot_state" "$workload_field")
            [ -n "$jit_workload_value" ] && [ "$jit_workload_value" = "$aot_workload_value" ] || {
                echo "unmatched workload identity for $program/$scenario/$workload_field" >&2
                exit 1
            }
        done
    done
done < "$CORPUS"

run_parity_checks

print_table() {
    printf 'compiler-speed version=%s corpus=%s corpus_sha256=%s stage=matrix machine=%s target=%s rustc=%s llvm=%s rustc_vv_sha256=%s rustc_sha256=%s compiler_sha256=%s jet_env_sha256=%s libc_sha256=%s allocator_sha256=%s allocator_environment_sha256=%s hardware_sha256=%s topology_sha256=%s toolchain_sha256=%s kernel=%s governor=%s memory_bytes=%s profiles=jit-fast,aot-release backends=cranelift,rustc-llvm warmups=%s samples=%s parity=%s parity_cases=%s\n' \
        "$REPORT_VERSION" \
        "$corpus_count" "$corpus_sha" "$machine" "$machine_target" "$machine_rustc" \
        "$machine_llvm" "$machine_rustc_vv_sha" "$machine_rustc_sha256" "$compiler_sha256" "$machine_jet_env_sha256" \
        "$machine_libc_sha256" "$machine_allocator_source_sha256" "$machine_allocator_environment_sha256" \
        "$machine_hardware_sha256" "$machine_topology_sha256" "$machine_toolchain_sha256" "$machine_kernel" \
        "$machine_governor" "$machine_memory" "$WARMUPS" "$SAMPLES" "$PARITY_RECEIPT" "$PARITY_CASE_COUNT"
    printf '%-54s %-25s %-16s %16s %16s %10s %-64s\n' \
        program state stage latency_ns memory_bytes variance_pct output_sha256:stderr_sha256
    while IFS="$TAB" read -r row_program row_state row_stage row_latency row_memory row_variance row_stdout_sha row_stderr_sha row_phases; do
        printf '%-54s %-25s %-16s %16s %16s %10s %s:%s phases=%s\n' \
            "$row_program" "$row_state" "$row_stage" "$row_latency" "$row_memory" "$row_variance" \
            "$row_stdout_sha" "$row_stderr_sha" "$row_phases"
    done < "$rows_file"
}

as_json() {
    printf '{"schema":"jet.compiler-speed","version":%s,"corpus_sha256":%s,"stage":"matrix",' "$REPORT_VERSION" "$(json_q "$corpus_sha")"
    printf '"parity":{"status":%s,"cases":%s,"semantic":%s,"diagnostics":%s,"effects":%s,"tiers":%s,"dev_profile":"dev","aot_profile":"release"},' \
        "$(json_q "$PARITY_RECEIPT")" "$PARITY_CASE_COUNT" "$(json_q "$PARITY_RECEIPT")" "$(json_q "$PARITY_RECEIPT")" "$(json_q "$PARITY_RECEIPT")" "$(json_q "$PARITY_RECEIPT")"
    printf '"machine":{"allocator":%s,"allocator_environment_sha256":%s,"allocator_source_sha256":%s,"arch":%s,"compiler_sha256":%s,"cpus":%s,"cpu_model":%s,"cpu_numa_nodes":%s,"cpu_online":%s,"cpu_cores_per_socket":%s,"cpu_sockets":%s,"cpu_threads_per_core":%s,"governor":%s,"hardware_sha256":%s,"hostname":%s,"kernel":%s,"libc_path":%s,"libc_sha256":%s,"libc_version":%s,"memory_bytes":%s,"os":%s,"rustc":%s,"rustc_path":%s,"rustc_sha256":%s,"rustc_vv_sha256":%s,"target":%s,"toolchain_sha256":%s,"topology_sha256":%s,"affinity":%s,"jet_env_sha256":%s},' \
        "$(json_q "$machine_allocator")" "$(json_q "$machine_allocator_environment_sha256")" "$(json_q "$machine_allocator_source_sha256")" \
        "$(json_q "$machine_arch")" "$(json_q "$compiler_sha256")" "$machine_cpus" "$(json_q "$machine_cpu_model")" \
        "$machine_cpu_numa_nodes" "$(json_q "$machine_cpu_online")" "$machine_cpu_cores_per_socket" "$machine_cpu_sockets" \
        "$machine_cpu_threads_per_core" "$(json_q "$machine_governor")" "$(json_q "$machine_hardware_sha256")" \
        "$(json_q "$machine_host")" "$(json_q "$machine_kernel")" "$(json_q "$machine_libc_path")" \
        "$(json_q "$machine_libc_sha256")" "$(json_q "$machine_libc_version")" "$machine_memory" "$(json_q "$machine_os")" \
        "$(json_q "$machine_rustc")" "$(json_q "$machine_rustc_path")" "$(json_q "$machine_rustc_sha256")" \
        "$(json_q "$machine_rustc_vv_sha")" "$(json_q "$machine_target")" "$(json_q "$machine_toolchain_sha256")" \
        "$(json_q "$machine_topology_sha256")" "$(json_q "$machine_affinity")" "$(json_q "$machine_jet_env_sha256")"
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
    --json)
        as_json
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
        echo "usage: tools/perf/dashboard.sh [--json|--baseline|--compare FILE|--environment|--construct-scale|--construct-scale-json]" >&2
        exit 2
        ;;
esac
