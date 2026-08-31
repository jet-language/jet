#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
checker_source="$script_dir/ci-perf-check.sh"
dashboard_source="$script_dir/dashboard.sh"
corpus_source="$script_dir/corpus.tsv"
scratch_parent=${JET_PERF_SCRATCH_ROOT:-"$HOME/.cache/jet-perf"}
scratch_parent_resolved=$(realpath -m -- "$scratch_parent")
case "$scratch_parent_resolved" in
    /tmp|/tmp/*|*/target|*/target/*)
        printf 'compiler-speed fixture scratch must be disk-backed and outside target: %s\n' "$scratch_parent" >&2
        exit 1
        ;;
esac
mkdir -p "$scratch_parent"
scratch_device=$(df -P "$scratch_parent" | awk 'NR == 2 { print $1; exit }')
case "$scratch_device" in
    ""|tmpfs|ramfs|none)
        printf 'compiler-speed fixture scratch must be disk-backed: %s\n' "$scratch_parent" >&2
        exit 1
        ;;
esac
root=$(mktemp -d "$scratch_parent/ci-perf-check.XXXXXX")
trap 'rm -rf "$root"' EXIT

fixture_perf="$root/tools/perf"
mkdir -p "$fixture_perf"
cp "$checker_source" "$fixture_perf/ci-perf-check.sh"
chmod +x "$fixture_perf/ci-perf-check.sh"
fixture_checker="$fixture_perf/ci-perf-check.sh"
fixture_baseline="$fixture_perf/baseline.json"
fixture_dashboard="$fixture_perf/dashboard.sh"

printf '%s\n' '#!/usr/bin/env sh
set -eu

mode=${FIXTURE_MODE:-pass}
manifest_identity=manifest
[ "$mode" = manifest ] && manifest_identity=changed
toolchain_identity=toolchain
[ "$mode" = toolchain ] && toolchain_identity=changed
[ "$mode" = outliers ] && {
    echo "bimodal compiler-speed benchmark: fixture.jet/jit-clean outliers=6 of 20 budget=5" >&2
    exit 1
}

TAB=$(printf "\\t")
semantic_parity=verified
[ "$mode" = parity ] && semantic_parity=unverified
variance=0
[ "$mode" = variance ] && variance=101
load_start=100
load_peak=200
load_end=150
[ "$mode" = load ] && load_peak=not-a-number
printf "%s\\n" "compiler-speed version=4 corpus=1 corpus_sha256=corpus manifest_sha256=$manifest_identity stage=matrix machine=Linux/x86_64/cpus=2/host=fixture target=target rustc=rustc llvm=llvm rustc_vv_sha256=rustc-vv rustc_sha256=rustc-bin compiler_sha256=compiler jet_env_sha256=jet-env libc_sha256=libc allocator_sha256=allocator allocator_environment_sha256=allocator-env hardware_sha256=hardware topology_sha256=topology toolchain_sha256=$toolchain_identity kernel=kernel governor=governor load1_start_milli=$load_start load1_peak_milli=$load_peak load1_end_milli=$load_end memory_bytes=1024 profiles=jit-fast,aot-release backends=cranelift,rustc-llvm warmups=1 samples=20 parity=verified parity_cases=1"
if [ "$mode" = row-format ]; then
    printf "%s\\n" "program state stage latency_ns memory_bytes variance_pct output_sha256:stderr_sha256 phases"
else
    printf "program\\tstate\\tstage\\tlatency_ns\\tmemory_bytes\\tvariance_pct\\toutput_sha256:stderr_sha256\\tphases\\n"
fi
for state in jit-clean jit-no-change jit-representative-edit aot-release-clean aot-release-no-change aot-release-representative-edit; do
    case "$state" in
        jit-*) stage=jit-fast; profile=fast; backend=cranelift; linker=none; cache_state=Clean; cache_policy=fresh-cache-per-sample ;;
        *) stage=aot-release; profile=release; backend=rustc-llvm; linker=ld; cache_state=Clean; cache_policy=fresh-cache-per-sample ;;
    esac
    case "$state" in
        *no-change) cache_state=NoChange; cache_policy=shared-cache-after-warmup ;;
        *representative-edit) cache_state=Edit; cache_policy=base-cache-snapshot-before-edit ;;
    esac
    phase="parse_us=1;sema_us=1;source=fixture.jet;source_sha256=source;source_bytes=1;expected_sha256=expected;expected_bytes=1;manifest_sha256=$manifest_identity;workload_sha256=workload;role=base;profile=$profile;backend=$backend;linker=$linker;linker_path=$linker;linker_sha256=$linker;linker_backend=$linker;linker_backend_path=$linker;linker_backend_sha256=$linker;cache_state=$cache_state;cache_policy=$cache_policy;cache_hits=1;cache_misses=0;libc_sha256=libc;allocator_sha256=allocator;allocator_environment_sha256=allocator-env;hardware_sha256=hardware;topology_sha256=topology;toolchain_sha256=$toolchain_identity;rustc_sha256=rustc-bin;top_cause=none;artifact_bytes=0;parity=verified;semantic_parity=$semantic_parity;diagnostic_parity=verified;effect_parity=verified;tier_parity=verified;dev_profile=dev;aot_profile=release"
    if [ "$mode" = row-format ]; then
        printf "%s\\n" "fixture.jet $state $stage 100 100 $variance stdout:stderr phases=$phase"
    else
        printf "%s\\t%s\\t%s\\t100\\t100\\t%s\\tstdout:stderr\\tphases=%s\\n" fixture.jet "$state" "$stage" "$variance" "$phase"
    fi
done
' > "$fixture_dashboard"
chmod +x "$fixture_dashboard"

phase_for_state() {
    state=$1
    case "$state" in
        jit-*) stage=jit-fast; profile=fast; backend=cranelift; linker=none; cache_state=Clean; cache_policy=fresh-cache-per-sample ;;
        *) stage=aot-release; profile=release; backend=rustc-llvm; linker=ld; cache_state=Clean; cache_policy=fresh-cache-per-sample ;;
    esac
    case "$state" in
        *no-change) cache_state=NoChange; cache_policy=shared-cache-after-warmup ;;
        *representative-edit) cache_state=Edit; cache_policy=base-cache-snapshot-before-edit ;;
    esac
    printf 'parse_us=1;sema_us=1;source=fixture.jet;source_sha256=source;source_bytes=1;expected_sha256=expected;expected_bytes=1;manifest_sha256=manifest;workload_sha256=workload;role=base;profile=%s;backend=%s;linker=%s;linker_path=%s;linker_sha256=%s;linker_backend=%s;linker_backend_path=%s;linker_backend_sha256=%s;cache_state=%s;cache_policy=%s;cache_hits=1;cache_misses=0;libc_sha256=libc;allocator_sha256=allocator;allocator_environment_sha256=allocator-env;hardware_sha256=hardware;topology_sha256=topology;toolchain_sha256=toolchain;rustc_sha256=rustc-bin;top_cause=none;artifact_bytes=0;parity=verified;semantic_parity=verified;diagnostic_parity=verified;effect_parity=verified;tier_parity=verified;dev_profile=dev;aot_profile=release\n' \
        "$profile" "$backend" "$linker" "$linker" "$linker" "$linker" "$linker" "$linker" "$cache_state" "$cache_policy"
}

write_baseline() {
    version=$1
    compiler_identity='"compiler_sha256":"compiler",'
    [ "${2:-present}" = missing ] && compiler_identity=
    runs=
    for state in jit-clean jit-no-change jit-representative-edit aot-release-clean aot-release-no-change aot-release-representative-edit; do
        case "$state" in
            jit-*) stage=jit-fast ;;
            *) stage=aot-release ;;
        esac
        phase=$(phase_for_state "$state")
        phase=${phase%$'\n'}
        row=$(printf '{"program":"fixture.jet","state":"%s","stage":"%s","latency_ns":100,"memory_bytes":100,"variance_pct":0,"stdout_sha256":"stdout","stderr_sha256":"stderr","phase_totals":"%s"}' "$state" "$stage" "$phase")
        [ -z "$runs" ] || runs="$runs,"
        runs="$runs$row"
    done
    printf '{"schema":"jet.compiler-speed","version":%s,"corpus_sha256":"corpus","manifest_sha256":"manifest","stage":"matrix","parity":{"status":"verified","cases":1,"semantic":"verified","diagnostics":"verified","effects":"verified","tiers":"verified"},"machine":{"allocator_source_sha256":"allocator","allocator_environment_sha256":"allocator-env","arch":"x86_64",%s"cpus":2,"governor":"governor","hardware_sha256":"hardware","hostname":"fixture","kernel":"kernel","libc_sha256":"libc","load1_start_milli":100,"load1_peak_milli":200,"load1_end_milli":150,"memory_bytes":1024,"os":"Linux","rustc":"rustc","rustc_vv_sha256":"rustc-vv","target":"target","toolchain_sha256":"toolchain","topology_sha256":"topology","jet_env_sha256":"jet-env","llvm":"llvm","rustc_sha256":"rustc-bin"},"budgets":{"latency_regression_pct":15,"memory_regression_pct":15,"samples":20,"variance_pct":100,"warmups":1},"runs":[%s]}\n' "$version" "$compiler_identity" "$runs" > "$fixture_baseline"
}

fixture_mode=pass
run_checker() {
    FIXTURE_MODE="$fixture_mode" \
        JET_CI_CANDIDATE_COMMIT=0123456789abcdef0123456789abcdef01234567 \
        "$fixture_checker"
}

expect_status() {
    want=$1
    label=$2
    needle=$3
    set +e
    output=$(run_checker 2>&1)
    status=$?
    set -e
    if [ "$status" -ne "$want" ]; then
        printf '%s: expected exit %s, got %s\n%s\n' "$label" "$want" "$status" "$output" >&2
        exit 1
    fi
    if [ -n "$needle" ] && ! printf '%s\n' "$output" | grep -Fq -- "$needle"; then
        printf '%s: missing %s\n%s\n' "$label" "$needle" "$output" >&2
        exit 1
    fi
}

assert_dashboard_source() {
    needle=$1
    grep -Fq -- "$needle" "$dashboard_source" || {
        printf 'dashboard gate removal detected: %s\n' "$needle" >&2
        exit 1
    }
}
assert_checker_source() {
    needle=$1
    grep -Fq -- "$needle" "$checker_source" || {
        printf 'CI scratch/receipt gate removal detected: %s\n' "$needle" >&2
        exit 1
    }
}

assert_corpus_order() {
    expected=$(printf '%s\n' \
        'examples/features/basics/hello.jet' \
        'examples/features/collections/wordcount.jet' \
        'examples/features/serde/json.jet' \
        'examples/features/basics/pattern_matching.jet' \
        'examples/features/devloop/job_runner.jet')
    actual=$(awk '
        $0 !~ /^[[:space:]]*(#|$)/ {
            sub(/[[:space:]].*/, "", $0)
            print
        }
    ' "$corpus_source")
    [ "$actual" = "$expected" ] || {
        printf 'compiler-speed corpus order changed\nexpected:\n%s\nactual:\n%s\n' "$expected" "$actual" >&2
        exit 1
    }
}

assert_corpus_order
assert_dashboard_source 'first_quartile = values[int((count + 3) / 4)]'
assert_dashboard_source 'third_quartile = values[int((3 * count + 1) / 4)]'
assert_dashboard_source 'fence = (third_quartile - first_quartile) * 3 / 2'
assert_dashboard_source 'state_variance=$(variance_file "$state_latency")'
assert_dashboard_source '[ "$state_variance" -gt "$VARIANCE_BUDGET_PCT" ]'
assert_dashboard_source 'state_outliers=$(outlier_file "$state_latency")'
assert_dashboard_source '[ "$state_outliers" -gt "$OUTLIER_BUDGET_COUNT" ]'
assert_dashboard_source 'FIXTURE_PACKAGE="$run_dir/package.jet"'
assert_dashboard_source 'manifest_sha256=$(sha256 "$FIXTURE_PACKAGE")'
assert_dashboard_source ';manifest_sha256=$manifest_sha256;'
assert_dashboard_source "ROW_HEADER=\$(printf 'program\\tstate\\tstage\\tlatency_ns\\tmemory_bytes\\tvariance_pct\\toutput_sha256:stderr_sha256\\tphases')"
assert_dashboard_source "printf '%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s:%s\\tphases=%s\\n'"
assert_dashboard_source 'parity_run_dev_case "edit-$(safe_id "$parity_edit")" "$parity_edit" "$parity_edit_expected" "$parity_program-jit-representative-edit"'
assert_dashboard_source 'job_argument_for_program()'
assert_dashboard_source 'examples/features/devloop/job_runner.jet|tools/perf/edits/job_runner.jet'
assert_dashboard_source '"$JET_BIN" run run.jet -- seed_data'
assert_dashboard_source '"$JET_BIN" run --interpret run.jet -- seed_data'
assert_dashboard_source '"$JET_BIN" jobs'
assert_dashboard_source 'parity_check_job_runner_case edit tools/perf/edits/job_runner.jet'
assert_dashboard_source 'TMP_ROOT=${JET_PERF_SCRATCH_ROOT:-"$HOME/.cache/jet-perf"}'
assert_dashboard_source 'run_dir=$(mktemp -d "$TMP_ROOT/compiler-speed.XXXXXX")'
assert_dashboard_source 'run_bounded_process()'
assert_dashboard_source 'record_machine_load'
assert_dashboard_source 'validate_sample_file()'
assert_dashboard_source 'reset_fixture_state()'
assert_dashboard_source 'cleanup_alive()'
assert_dashboard_source 'df -P "$TMP_ROOT"'
assert_dashboard_source 'LC_ALL=C'
assert_dashboard_source 'JET_TIMING=1'
assert_dashboard_source 'kill -KILL "-$ACTIVE_PID"'
assert_dashboard_source 'cp -R "$edit_warm_cache" "$edit_cache"'
assert_dashboard_source 'reset_fixture_state "$edit_work"'
assert_dashboard_source 'load1_start_milli'
assert_dashboard_source 'load1_peak_milli'
assert_checker_source 'realpath -m -- "$SCRATCH_ROOT"'
assert_checker_source 'df -P "$SCRATCH_ROOT"'
assert_checker_source 'SCRATCH_ROOT=${JET_PERF_SCRATCH_ROOT:-"$HOME/.cache/jet-perf"}'
assert_checker_source 'CI_RUN_DIR=$(mktemp -d "$SCRATCH_ROOT/compiler-speed-ci.XXXXXX")'
assert_checker_source 'cleanup_ci()'
assert_checker_source 'JET_PERF_SCRATCH_ROOT="$SCRATCH_ROOT" TMPDIR="$SCRATCH_ROOT"'

workload_function=$(sed -n '/^workload_digest() {/,/^}/p' "$dashboard_source")
[ -n "$workload_function" ] || {
    printf 'dashboard workload identity function missing\n' >&2
    exit 1
}
sha256_text() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | awk '{print $1}'
    else
        shasum -a 256 | awk '{print $1}'
    fi
}
sample_validation_function=$(sed -n '/^validate_sample_file() {/,/^}/p' "$dashboard_source")
variance_function=$(sed -n '/^variance_file() {/,/^}/p' "$dashboard_source")
outlier_function=$(sed -n '/^outlier_file() {/,/^}/p' "$dashboard_source")
[ -n "$sample_validation_function" ] && [ -n "$variance_function" ] && [ -n "$outlier_function" ] || {
    printf 'dashboard dispersion functions missing\n' >&2
    exit 1
}
eval "$workload_function"
eval "$sample_validation_function"
eval "$variance_function"
eval "$outlier_function"
manifest_workload=$(workload_digest fixture.jet base source expected 1 1 manifest-a toolchain)
changed_manifest_workload=$(workload_digest fixture.jet base source expected 1 1 manifest-b toolchain)
toolchain_workload=$(workload_digest fixture.jet base source expected 1 1 manifest toolchain-b)
[ "$manifest_workload" != "$changed_manifest_workload" ] || {
    printf 'manifest edit did not change workload identity\n' >&2
    exit 1
}
[ "$manifest_workload" != "$toolchain_workload" ] || {
    printf 'toolchain edit did not change workload identity\n' >&2
    exit 1
}

SAMPLES=20
variance_samples="$root/variance.samples"
printf '%s\n' 100 100 100 100 100 200 200 200 200 200 400 400 400 400 400 500 500 500 500 500 > "$variance_samples"
[ "$(variance_file "$variance_samples")" -gt 100 ] || {
    printf 'IQR fixture did not exceed budget\n' >&2
    exit 1
}
outlier_samples="$root/outlier.samples"
: > "$outlier_samples"
for value in 1 2 3; do printf '100\n' >> "$outlier_samples"; done
for value in 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do printf '200\n' >> "$outlier_samples"; done
for value in 1 2 3; do printf '300\n' >> "$outlier_samples"; done
outlier_count=$(outlier_file "$outlier_samples")
outlier_budget=$(sed -n 's/^OUTLIER_BUDGET_COUNT=//p' "$dashboard_source")
[ "$outlier_count" -gt "$outlier_budget" ] || {
    printf 'Tukey fixture did not find six outliers\n' >&2
    exit 1
}
invalid_samples="$root/invalid.samples"
printf '%s\n' 100 100 100 100 100 100 100 100 100 100 100 100 100 100 100 100 100 100 100 bad > "$invalid_samples"
set +e
invalid_output=$(variance_file "$invalid_samples" 2>&1)
invalid_status=$?
set -e
[ "$invalid_status" -ne 0 ] && [[ "$invalid_output" == *"invalid compiler-speed sample file"* ]] || {
    printf 'malformed sample fixture did not fail closed\n' >&2
    exit 1
}


write_baseline 4
expect_status 0 'valid v4 report' 'perf gate OK'

fixture_mode=load
expect_status 1 'invalid machine load' 'invalid compiler-speed machine load accounting'
fixture_mode=pass
fixture_mode=row-format
expect_status 1 'noncanonical rows' 'invalid compiler-speed row header'

fixture_mode=manifest
expect_status 1 'manifest identity mismatch' 'package-manifest changed: manifest -> changed'

fixture_mode=toolchain
expect_status 1 'toolchain identity mismatch' 'toolchain changed: toolchain -> changed'

write_baseline 2
expect_status 1 'stale schema' 'unsupported compiler-speed baseline version: 2'

write_baseline 4 missing
expect_status 1 'missing identity' 'baseline has incomplete corpus/stage/machine/budget identity'

write_baseline 4
fixture_mode=variance
expect_status 1 'excessive IQR' 'UNSTABLE fixture.jet/jit-clean interquartile spread=101% budget=100%'

fixture_mode=outliers
expect_status 1 'excessive Tukey outliers' 'bimodal compiler-speed benchmark'

fixture_mode=parity
expect_status 1 'parity mismatch' 'missing JIT/dev/AOT semantic parity receipt'

printf 'ci-perf-check self-check: all pass\n'
