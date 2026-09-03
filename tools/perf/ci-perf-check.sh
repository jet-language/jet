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
PEER_META_PREFIX='compiler-speed-peer version='
PEER_ROW_HEADER=$(printf 'peer\tlanguage\tprogram\tstate\tmetric\tvalue\tworkload_sha256\tsource_sha256\texpected_sha256\tmanifest_sha256\ttoolchain_sha256')
PEER_METRICS=latency_ns,memory_bytes
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

sha256_text() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum | awk '{print $1}'
    else
        shasum -a 256 | awk '{print $1}'
    fi
}

is_sha256() {
    hash_value=$1
    case "$hash_value" in
        ''|*[!0-9a-fA-F]*) return 1 ;;
    esac
    [ "${#hash_value}" -eq 64 ]
}

is_positive_integer() {
    integer_value=$1
    case "$integer_value" in
        ''|0|*[!0-9]*) return 1 ;;
    esac
    return 0
}
is_nonnegative_integer() {
    integer_value=$1
    case "$integer_value" in
        ''|*[!0-9]*) return 1 ;;
    esac
    return 0
}
is_pending_peer_contract() {
    [ "$1" = pending-D-BUILDBENCH1 ] &&
        [ "$2" = pending:D-BUILDBENCH1 ] &&
        [ "$3" = 0 ] &&
        [ "$4" = 0 ]
}



SUPPORTED_GENERATOR_VERSION=1
MAIN_META_KEYS='version generator_version corpus corpus_sha256 manifest_sha256 stage machine target rustc cargo llvm rustc_vv_sha256 rustc_sha256 compiler_sha256 jet_commit jet_env_sha256 libc_sha256 allocator_sha256 allocator_environment_sha256 hardware_sha256 topology_sha256 toolchain_sha256 kernel governor cpu_model cpu_cores_per_socket load1_start_milli load1_peak_milli load1_end_milli memory_bytes profiles backends warmups samples outliers_discarded parity parity_cases peer_contract_sha256 peer_run_id peer_machine peer_target peer_keys peer_metrics peer_count peer_rows'
LEGACY_MAIN_META_KEYS='version corpus corpus_sha256 manifest_sha256 stage machine target rustc llvm rustc_vv_sha256 rustc_sha256 compiler_sha256 jet_env_sha256 libc_sha256 allocator_sha256 allocator_environment_sha256 hardware_sha256 topology_sha256 toolchain_sha256 kernel governor load1_start_milli load1_peak_milli load1_end_milli memory_bytes profiles backends warmups samples outliers_discarded parity parity_cases peer_contract_sha256 peer_run_id peer_machine peer_target peer_keys peer_metrics peer_count peer_rows'
PEER_META_KEYS='version run_id corpus_sha256 manifest_sha256 target machine peers metrics contract_sha256 peer_count rows'


baseline_schema=$(json_string schema)
baseline_corpus=$(json_string corpus_sha256)
baseline_manifest=$(json_string manifest_sha256)
baseline_version=$(json_number version)
baseline_generator_version=$(json_number generator_version)
baseline_stage=$(json_string stage)
baseline_os=$(json_string os)
baseline_arch=$(json_string arch)
baseline_target=$(json_string target)
baseline_rustc=$(json_string rustc)
baseline_cargo=$(json_string cargo)
baseline_llvm=$(json_string llvm)
baseline_rustc_vv=$(json_string rustc_vv_sha256)
baseline_rustc_sha=$(json_string rustc_sha256)
baseline_compiler=$(json_string compiler_sha256)
baseline_jet_env=$(json_string jet_env_sha256)
baseline_jet_commit=$(json_string jet_commit)
baseline_libc=$(json_string libc_sha256)
baseline_allocator=$(json_string allocator_source_sha256)
baseline_allocator_environment=$(json_string allocator_environment_sha256)
baseline_hardware=$(json_string hardware_sha256)
baseline_topology=$(json_string topology_sha256)
baseline_toolchain=$(json_string toolchain_sha256)
baseline_kernel=$(json_string kernel)
baseline_governor=$(json_string governor)
baseline_cpu_model=$(json_string cpu_model | sed 's/[[:space:]]/_/g')
baseline_cpu_cores=$(json_number cpu_cores_per_socket)
baseline_cpus=$(json_number cpus)
baseline_memory=$(json_number memory_bytes)
baseline_load_start=$(json_number load1_start_milli)
baseline_load_peak=$(json_number load1_peak_milli)
baseline_load_end=$(json_number load1_end_milli)
baseline_host=$(json_string hostname)
baseline_machine="$baseline_os/$baseline_arch/cpus=$baseline_cpus/host=$baseline_host"
latency_budget=$(json_number latency_regression_pct)
memory_budget=$(json_number memory_regression_pct)
variance_budget=$(json_number variance_pct)
baseline_samples=$(json_number samples)
baseline_warmups=$(json_number warmups)
baseline_parity=$(json_string status)
baseline_parity_cases=$(json_number cases)
baseline_semantic_parity=$(json_string semantic)
baseline_peer_version=$(json_number peer_version)
baseline_peer_contract=$(json_string peer_contract_sha256)
baseline_peer_keys=$(json_string peer_keys)
baseline_peer_metrics=$(json_string peer_metrics)
baseline_peer_count=$(json_number peer_count)
baseline_peer_rows=$(json_number peer_rows)
baseline_peer_run_id=$(json_string peer_run_id)
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
if [ -n "$baseline_generator_version" ] || [ -n "$baseline_cargo" ] || \
    [ -n "$baseline_jet_commit" ] || [ -n "$baseline_cpu_model" ] || [ -n "$baseline_cpu_cores" ]; then
    for value in "$baseline_generator_version" "$baseline_cargo" "$baseline_jet_commit" \
        "$baseline_cpu_model" "$baseline_cpu_cores"; do
        [ -n "$value" ] || {
            echo "baseline has incomplete compiler-speed generator identity" >&2
            exit 1
        }
    done
    [ "$baseline_generator_version" -eq "$SUPPORTED_GENERATOR_VERSION" ] || {
        echo "unsupported compiler-speed generator version: $baseline_generator_version" >&2
        exit 1
    }
    is_positive_integer "$baseline_cpu_cores" || {
        echo "baseline has invalid CPU core-count identity" >&2
        exit 1
    }
    case "$baseline_jet_commit" in
        ''|*[!0-9a-fA-F]*) echo "baseline has invalid Jet commit identity" >&2; exit 1 ;;
    esac
    [ "${#baseline_jet_commit}" -eq 40 ] || {
        echo "baseline has invalid Jet commit identity" >&2
        exit 1
    }
fi
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
for value in "$baseline_peer_version" "$baseline_peer_contract" "$baseline_peer_keys" "$baseline_peer_metrics" "$baseline_peer_count" "$baseline_peer_rows" "$baseline_peer_run_id"; do
    [ -n "$value" ] || { echo "baseline has incomplete compiler-speed peer contract" >&2; exit 1; }
done
[ "$baseline_peer_version" -eq 1 ] || {
    echo "unsupported compiler-speed peer contract version: $baseline_peer_version" >&2
    exit 1
}
[ "$baseline_peer_metrics" = "$PEER_METRICS" ] || {
    echo "unsupported compiler-speed peer metrics: $baseline_peer_metrics" >&2
    exit 1
}
is_sha256 "$baseline_peer_contract" || {
    echo "baseline has invalid compiler-speed peer contract digest" >&2
    exit 1
}
baseline_peer_pending=0
if is_pending_peer_contract "$baseline_peer_run_id" "$baseline_peer_keys" "$baseline_peer_count" "$baseline_peer_rows"; then
    baseline_peer_pending=1
else
    is_positive_integer "$baseline_peer_count" || {
        echo "baseline has invalid compiler-speed peer count" >&2
        exit 1
    }
    is_positive_integer "$baseline_peer_rows" || {
        echo "baseline has invalid compiler-speed peer coverage count" >&2
        exit 1
    }
    baseline_named_peer_count=$(printf '%s\n' "$baseline_peer_keys" | awk -F, '
        {
            if (NF == 0) invalid = 1
            for (i = 1; i <= NF; i++) {
                if (split($i, pair, ":") != 2 ||
                    pair[1] !~ /^[A-Za-z0-9_.-]+$/ ||
                    pair[2] !~ /^[A-Za-z0-9_.-]+$/ ||
                    seen_peer[pair[1]]++) {
                    invalid = 1
                }
                if (pair[2] == "rust") rust_peer = 1
            }
        }
        END {
            if (invalid || !rust_peer) exit 1
            print NF
        }
    ') || {
        echo "baseline has invalid compiler-speed peer declarations" >&2
        exit 1
    }
    [ "$baseline_named_peer_count" -eq "$baseline_peer_count" ] || {
        echo "baseline peer declaration count mismatch: declared $baseline_peer_count, named $baseline_named_peer_count" >&2
        exit 1
    }
fi
baseline_peer_contract_expected=$(printf 'jet.compiler-speed.peer.v1\ncorpus_sha256=%s\nmanifest_sha256=%s\ntarget=%s\nmachine=%s\npeers=%s\nmetrics=%s\n' \
    "$baseline_corpus" "$baseline_manifest" "$baseline_target" "$baseline_machine" "$baseline_peer_keys" "$baseline_peer_metrics" | sha256_text)
[ "$baseline_peer_contract" = "$baseline_peer_contract_expected" ] || {
    echo "baseline compiler-speed peer contract digest mismatch" >&2
    exit 1
}

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
metadata_has_keys() {
    expected_keys=$1
    printf '%s\n' "$metadata" | awk -v expected_keys="$expected_keys" '
        BEGIN { key_count = split(expected_keys, keys, " ") }
        {
            if ($1 != "compiler-speed" || NF != key_count + 1) invalid = 1
            for (i = 1; i <= key_count; i++) {
                if (index($(i + 1), keys[i] "=") != 1) invalid = 1
            }
        }
        END { if (invalid || NR != 1) exit 1 }
    '
}
metadata_has_keys "$MAIN_META_KEYS" || metadata_has_keys "$LEGACY_MAIN_META_KEYS" || {
    echo "invalid compiler-speed report metadata" >&2
    exit 1
}

metadata_value() {
    metadata_key=$1
    printf '%s\n' "$metadata" | awk -v key="$metadata_key" '
        {
            for (field = 1; field <= NF; field++) {
                if (index($field, key "=") == 1) {
                    value = $field
                    sub("^[^=]*=", "", value)
                    print value
                    exit
                }
            }
        }
    '
}
current_version=$(metadata_value version)
current_generator_version=$(metadata_value generator_version)
current_corpus_count=$(metadata_value corpus)
current_corpus=$(metadata_value corpus_sha256)
current_manifest=$(metadata_value manifest_sha256)
current_stage=$(metadata_value stage)
current_machine=$(metadata_value machine)
current_target=$(metadata_value target)
current_rustc=$(metadata_value rustc)
current_cargo=$(metadata_value cargo)
current_llvm=$(metadata_value llvm)
current_rustc_vv=$(metadata_value rustc_vv_sha256)
current_rustc_sha=$(metadata_value rustc_sha256)
current_compiler=$(metadata_value compiler_sha256)
current_jet_commit=$(metadata_value jet_commit)
current_jet_env=$(metadata_value jet_env_sha256)
current_libc=$(metadata_value libc_sha256)
current_allocator=$(metadata_value allocator_sha256)
current_allocator_environment=$(metadata_value allocator_environment_sha256)
current_hardware=$(metadata_value hardware_sha256)
current_topology=$(metadata_value topology_sha256)
current_toolchain=$(metadata_value toolchain_sha256)
current_kernel=$(metadata_value kernel)
current_governor=$(metadata_value governor)
current_cpu_model=$(metadata_value cpu_model)
current_cpu_cores=$(metadata_value cpu_cores_per_socket)
current_load_start=$(metadata_value load1_start_milli)
current_load_peak=$(metadata_value load1_peak_milli)
current_load_end=$(metadata_value load1_end_milli)
current_memory=$(metadata_value memory_bytes)
current_samples=$(metadata_value samples)
current_warmups=$(metadata_value warmups)
current_parity=$(metadata_value parity)
current_parity_cases=$(metadata_value parity_cases)
peer_metadata_count=$(printf '%s\n' "$CURRENT" | awk '/^compiler-speed-peer version=/{count++} END { print count + 0 }')
[ "$peer_metadata_count" -eq 1 ] || {
    echo "current report has no unique compiler-speed peer contract" >&2
    exit 1
}
peer_metadata=$(printf '%s\n' "$CURRENT" | sed -n '/^compiler-speed-peer version=/p')
peer_row_header_count=$(printf '%s\n' "$CURRENT" | awk -v header="$PEER_ROW_HEADER" '$0 == header { count++ } END { print count + 0 }')
[ "$peer_row_header_count" -eq 1 ] || {
    echo "current report has no unique compiler-speed peer row header" >&2
    exit 1
}
printf '%s\n' "$peer_metadata" | awk -v expected_keys="$PEER_META_KEYS" '
    BEGIN { key_count = split(expected_keys, keys, " ") }
    {
        if ($1 != "compiler-speed-peer" || NF != key_count + 1) invalid = 1
        for (i = 1; i <= key_count; i++) {
            if (index($(i + 1), keys[i] "=") != 1) invalid = 1
        }
    }
    END { if (invalid || NR != 1) exit 1 }
' || {
    echo "invalid compiler-speed peer metadata" >&2
    exit 1
}

current_peer_version=$(printf '%s\n' "$peer_metadata" | sed -n 's/^compiler-speed-peer version=\([^ ]*\).*$/\1/p')
current_peer_run_id=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* run_id=\([^ ]*\).*/\1/p')
current_peer_corpus=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* corpus_sha256=\([^ ]*\).*/\1/p')
current_peer_manifest=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* manifest_sha256=\([^ ]*\).*/\1/p')
current_peer_target=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* target=\([^ ]*\).*/\1/p')
current_peer_machine=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* machine=\([^ ]*\).*/\1/p')
current_peer_keys=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* peers=\([^ ]*\).*/\1/p')
current_peer_metrics=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* metrics=\([^ ]*\).*/\1/p')
current_peer_contract=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* contract_sha256=\([^ ]*\).*/\1/p')
current_peer_rows=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* rows=\([^ ]*\).*/\1/p')
current_peer_count=$(printf '%s\n' "$peer_metadata" | sed -n 's/.* peer_count=\([^ ]*\).*/\1/p')
current_os=$(printf '%s\n' "$current_machine" | cut -d/ -f1)
current_arch=$(printf '%s\n' "$current_machine" | cut -d/ -f2)
current_cpus=$(printf '%s\n' "$current_machine" | sed 's/.*cpus=\([^/]*\).*/\1/')
current_host=$(printf '%s\n' "$current_machine" | sed 's/.*host=//')
if [ -n "$current_generator_version" ] || [ -n "$current_cargo" ] || \
    [ -n "$current_jet_commit" ] || [ -n "$current_cpu_model" ] || [ -n "$current_cpu_cores" ]; then
    for value in "$current_generator_version" "$current_cargo" "$current_jet_commit" \
        "$current_cpu_model" "$current_cpu_cores"; do
        [ -n "$value" ] || {
            echo "current report has incomplete compiler-speed generator identity" >&2
            exit 1
        }
    done
    [ "$current_generator_version" -eq "$SUPPORTED_GENERATOR_VERSION" ] || {
        echo "unsupported compiler-speed generator version: $current_generator_version" >&2
        exit 1
    }
    is_positive_integer "$current_cpu_cores" || {
        echo "invalid current CPU core-count identity" >&2
        exit 1
    }
    case "$current_jet_commit" in
        ''|*[!0-9a-fA-F]*) echo "invalid current Jet commit identity" >&2; exit 1 ;;
    esac
    [ "${#current_jet_commit}" -eq 40 ] || {
        echo "invalid current Jet commit identity" >&2
        exit 1
    }
fi

for value in "$current_load_start" "$current_load_peak" "$current_load_end"; do
    is_nonnegative_integer "$value" || {
        echo "invalid compiler-speed machine load accounting" >&2
        exit 1
    }
done
[ "$current_load_peak" -ge "$current_load_start" ] && [ "$current_load_peak" -ge "$current_load_end" ] || {
    echo "invalid compiler-speed machine load peak" >&2
    exit 1
}
[ "$current_peer_version" = 1 ] || {
    echo "unsupported compiler-speed peer report version: ${current_peer_version:-missing}" >&2
    exit 1
}
for value in "$current_peer_run_id" "$current_peer_target" "$current_peer_machine" "$current_peer_keys" "$current_peer_metrics"; do
    [ -n "$value" ] || { echo "current report has incomplete compiler-speed peer contract" >&2; exit 1; }
done
for peer_hash in "$current_peer_corpus" "$current_peer_manifest" "$current_peer_contract"; do
    is_sha256 "$peer_hash" || {
        echo "current report has invalid compiler-speed peer contract identity" >&2
        exit 1
    }
done
current_peer_pending=0
if is_pending_peer_contract "$current_peer_run_id" "$current_peer_keys" "$current_peer_count" "$current_peer_rows"; then
    current_peer_pending=1
else
    is_positive_integer "$current_peer_count" || {
        echo "current report has invalid compiler-speed peer count" >&2
        exit 1
    }
    is_positive_integer "$current_peer_rows" || {
        echo "current report has invalid compiler-speed peer coverage count" >&2
        exit 1
    }
    current_named_peer_count=$(printf '%s\n' "$current_peer_keys" | awk -F, '
        {
            if (NF == 0) invalid = 1
            for (i = 1; i <= NF; i++) {
                if (split($i, pair, ":") != 2 ||
                    pair[1] !~ /^[A-Za-z0-9_.-]+$/ ||
                    pair[2] !~ /^[A-Za-z0-9_.-]+$/ ||
                    seen_peer[pair[1]]++) {
                    invalid = 1
                }
                if (pair[2] == "rust") rust_peer = 1
            }
        }
        END {
            if (invalid || !rust_peer) exit 1
            print NF
        }
    ') || {
        echo "current report has invalid compiler-speed peer declarations" >&2
        exit 1
    }
    [ "$current_named_peer_count" -eq "$current_peer_count" ] || {
        echo "current peer declaration count mismatch: declared $current_peer_count, named $current_named_peer_count" >&2
        exit 1
    }
fi
current_peer_contract_expected=$(printf 'jet.compiler-speed.peer.v1\ncorpus_sha256=%s\nmanifest_sha256=%s\ntarget=%s\nmachine=%s\npeers=%s\nmetrics=%s\n' \
    "$current_peer_corpus" "$current_peer_manifest" "$current_peer_target" "$current_peer_machine" "$current_peer_keys" "$current_peer_metrics" | sha256_text)
[ "$current_peer_contract" = "$current_peer_contract_expected" ] || {
    echo "current compiler-speed peer contract digest mismatch" >&2
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
check_optional_identity() {
    identity_name=$1
    identity_current=$2
    identity_baseline=$3
    if [ -n "$identity_current" ] || [ -n "$identity_baseline" ]; then
        [ -n "$identity_current" ] && [ -n "$identity_baseline" ] || {
            echo "$identity_name identity missing" >&2
            exit 1
        }
        check_identity "$identity_name" "$identity_current" "$identity_baseline"
    fi
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
check_optional_identity generator-version "$current_generator_version" "$baseline_generator_version"
check_optional_identity cargo-version "$current_cargo" "$baseline_cargo"
check_optional_identity Jet-commit "$current_jet_commit" "$baseline_jet_commit"
check_optional_identity CPU-model "$current_cpu_model" "$baseline_cpu_model"
check_optional_identity CPU-core-count "$current_cpu_cores" "$baseline_cpu_cores"
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
check_identity peer-version "$current_peer_version" "$baseline_peer_version"
check_identity peer-contract "$current_peer_contract" "$baseline_peer_contract"
check_identity peer-keys "$current_peer_keys" "$baseline_peer_keys"
check_identity peer-metrics "$current_peer_metrics" "$baseline_peer_metrics"
check_identity peer-count "$current_peer_count" "$baseline_peer_count"
check_identity peer-machine "$current_peer_machine" "$baseline_machine"

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
json_object_value() {
    json_object=$1
    json_field_name=$2
    printf '%s\n' "$json_object" \
        | sed -n 's/.*"'"$json_field_name"'":\("[^"]*"\|[0-9][0-9]*\).*/\1/p' \
        | sed 's/^"//; s/"$//'
}

baseline_peer_field() {
    field_peer=$1
    field_language=$2
    field_program=$3
    field_state=$4
    field_metric=$5
    field_name=$6
    baseline_peer_object=$(grep -F \
        '"peer":"'"$field_peer"'","language":"'"$field_language"'","program":"'"$field_program"'","state":"'"$field_state"'","metric":"'"$field_metric"'"' \
        "$BASELINE_PEER_ROWS" | head -n1 || true)
    [ -n "$baseline_peer_object" ] || return 0
    json_object_value "$baseline_peer_object" "$field_name"
}

peer_declared() {
    peer_declarations=$1
    peer_wanted=$2
    printf '%s\n' "$peer_declarations" | awk -F, -v wanted="$peer_wanted" '
        { for (i = 1; i <= NF; i++) if ($i == wanted) found = 1 }
        END { exit !found }
    '
}

check_baseline_peer_rows() {
    baseline_actual_peer_rows=$(awk 'NF { count++ } END { print count + 0 }' "$BASELINE_PEER_ROWS")
    [ "$baseline_actual_peer_rows" -eq "$baseline_peer_rows" ] || {
        echo "baseline peer array row count changed: expected $baseline_peer_rows, got $baseline_actual_peer_rows" >&2
        exit 1
    }
    if [ "$baseline_peer_pending" -eq 1 ]; then
        echo "baseline peer gate pending D-BUILDBENCH1" >&2
        return 0
    fi
    baseline_peer_keys_seen="$CI_RUN_DIR/baseline.peer.keys"
    baseline_peer_toolchains="$CI_RUN_DIR/baseline.peer.toolchains"
    : > "$baseline_peer_keys_seen"
    : > "$baseline_peer_toolchains"
    while IFS= read -r baseline_peer_object; do
        [ -n "$baseline_peer_object" ] || continue
        baseline_peer_name=$(json_object_value "$baseline_peer_object" peer)
        baseline_peer_language=$(json_object_value "$baseline_peer_object" language)
        baseline_peer_program=$(json_object_value "$baseline_peer_object" program)
        baseline_peer_state=$(json_object_value "$baseline_peer_object" state)
        baseline_peer_metric=$(json_object_value "$baseline_peer_object" metric)
        baseline_peer_value=$(json_object_value "$baseline_peer_object" value)
        baseline_peer_workload=$(json_object_value "$baseline_peer_object" workload_sha256)
        baseline_peer_source=$(json_object_value "$baseline_peer_object" source_sha256)
        baseline_peer_expected=$(json_object_value "$baseline_peer_object" expected_sha256)
        baseline_peer_manifest_row=$(json_object_value "$baseline_peer_object" manifest_sha256)
        baseline_peer_toolchain=$(json_object_value "$baseline_peer_object" toolchain_sha256)
        for value in "$baseline_peer_name" "$baseline_peer_language" "$baseline_peer_program" \
            "$baseline_peer_state" "$baseline_peer_metric" "$baseline_peer_value" "$baseline_peer_workload" \
            "$baseline_peer_source" "$baseline_peer_expected" "$baseline_peer_manifest_row" "$baseline_peer_toolchain"; do
            [ -n "$value" ] || {
                echo "baseline has incomplete compiler-speed peer row" >&2
                exit 1
            }
        done
        peer_declared "$baseline_peer_keys" "$baseline_peer_name:$baseline_peer_language" || {
            echo "baseline compiler-speed peer row names undeclared peer: $baseline_peer_name/$baseline_peer_language" >&2
            exit 1
        }
        case "$baseline_peer_program:$baseline_peer_state" in
            *[!A-Za-z0-9_./:-]*) echo "baseline has invalid compiler-speed peer workload key" >&2; exit 1 ;;
        esac
        case "$baseline_peer_metric" in
            latency_ns|memory_bytes) ;;
            *) echo "baseline has invalid compiler-speed peer metric: $baseline_peer_metric" >&2; exit 1 ;;
        esac
        is_positive_integer "$baseline_peer_value" || {
            echo "baseline has invalid compiler-speed peer value: $baseline_peer_name/$baseline_peer_program/$baseline_peer_state/$baseline_peer_metric" >&2
            exit 1
        }
        for peer_hash in "$baseline_peer_workload" "$baseline_peer_source" "$baseline_peer_expected" \
            "$baseline_peer_manifest_row" "$baseline_peer_toolchain"; do
            is_sha256 "$peer_hash" || {
                echo "baseline has invalid compiler-speed peer input identity" >&2
                exit 1
            }
        done
        [ "$baseline_peer_manifest_row" = "$baseline_manifest" ] || {
            echo "baseline compiler-speed peer manifest mismatch: $baseline_peer_program/$baseline_peer_state" >&2
            exit 1
        }
        baseline_peer_row_key=$(printf '%s\t%s\t%s\t%s\t%s' "$baseline_peer_name" "$baseline_peer_language" \
            "$baseline_peer_program" "$baseline_peer_state" "$baseline_peer_metric")
        if grep -Fqx -- "$baseline_peer_row_key" "$baseline_peer_keys_seen"; then
            echo "duplicate baseline compiler-speed peer row: $baseline_peer_row_key" >&2
            exit 1
        fi
        printf '%s\n' "$baseline_peer_row_key" >> "$baseline_peer_keys_seen"
        printf '%s:%s\t%s\n' "$baseline_peer_name" "$baseline_peer_language" "$baseline_peer_toolchain" >> "$baseline_peer_toolchains"
        baseline_jet_stage=$(baseline_field "$baseline_peer_program" "$baseline_peer_state" stage)
        baseline_jet_phases=$(baseline_field "$baseline_peer_program" "$baseline_peer_state" phase_totals)
        [ -n "$baseline_jet_stage" ] && [ -n "$baseline_jet_phases" ] || {
            echo "baseline compiler-speed peer row names unknown Jet row: $baseline_peer_program/$baseline_peer_state" >&2
            exit 1
        }
        baseline_jet_workload=$(phase_value "$baseline_jet_phases" workload_sha256)
        baseline_jet_source=$(phase_value "$baseline_jet_phases" source_sha256)
        baseline_jet_expected=$(phase_value "$baseline_jet_phases" expected_sha256)
        [ "$baseline_peer_workload" = "$baseline_jet_workload" ] || {
            echo "baseline compiler-speed peer workload identity mismatch: $baseline_peer_name/$baseline_peer_program/$baseline_peer_state" >&2
            exit 1
        }
        [ "$baseline_peer_source" = "$baseline_jet_source" ] && [ "$baseline_peer_expected" = "$baseline_jet_expected" ] || {
            echo "baseline compiler-speed peer input identity mismatch: $baseline_peer_name/$baseline_peer_program/$baseline_peer_state" >&2
            exit 1
        }
        case "$baseline_peer_metric" in
            latency_ns) baseline_jet_value=$(baseline_field "$baseline_peer_program" "$baseline_peer_state" latency_ns) ;;
            memory_bytes) baseline_jet_value=$(baseline_field "$baseline_peer_program" "$baseline_peer_state" memory_bytes) ;;
        esac
        is_positive_integer "$baseline_jet_value" || {
            echo "baseline has invalid Jet compiler-speed cell: $baseline_peer_program/$baseline_peer_state/$baseline_peer_metric" >&2
            exit 1
        }
        if [ "$baseline_peer_language" = rust ]; then
            awk -v jet="$baseline_jet_value" -v peer="$baseline_peer_value" 'BEGIN { exit !(jet / peer <= 1.05) }' || {
                echo "baseline compiler-speed peer loss: $baseline_peer_name/$baseline_peer_program/$baseline_peer_state/$baseline_peer_metric" >&2
                exit 1
            }
        else
            awk -v jet="$baseline_jet_value" -v peer="$baseline_peer_value" 'BEGIN { exit !(jet / peer < 1.00) }' || {
                echo "baseline compiler-speed peer loss: $baseline_peer_name/$baseline_peer_program/$baseline_peer_state/$baseline_peer_metric" >&2
                exit 1
            }
        fi
    done < "$BASELINE_PEER_ROWS"
    awk -F "$TAB" '
        {
            if (toolchain[$1] == "") toolchain[$1] = $2
            else if (toolchain[$1] != $2) invalid = 1
        }
        END { exit invalid }
    ' "$baseline_peer_toolchains" || {
        echo "baseline compiler-speed peer toolchain identity changed within a peer" >&2
        exit 1
    }
    baseline_expected_peer_keys="$CI_RUN_DIR/baseline.expected.peer.keys"
    baseline_matrix_rows="$CI_RUN_DIR/baseline.matrix.rows"
    sed 's/],"peers":\[.*//' "$BASELINE" | sed 's/},{/}\n{/g' \
        | sed -n 's/.*"program":"\([^"]*\)","state":"\([^"]*\)".*/\1\t\2/p' > "$baseline_matrix_rows"
    awk -F "$TAB" -v declarations="$baseline_peer_keys" '
        BEGIN {
            peer_count = split(declarations, peers, ",")
            metric_count = split("latency_ns,memory_bytes", metrics, ",")
        }
        {
            for (peer_index = 1; peer_index <= peer_count; peer_index++) {
                split(peers[peer_index], pair, ":")
                for (metric_index = 1; metric_index <= metric_count; metric_index++)
                    printf "%s\t%s\t%s\t%s\t%s\n", pair[1], pair[2], $1, $2, metrics[metric_index]
            }
        }
    ' "$baseline_matrix_rows" > "$baseline_expected_peer_keys"
    sort "$baseline_expected_peer_keys" > "$CI_RUN_DIR/baseline.expected.peer.keys.sorted"
    sort "$baseline_peer_keys_seen" > "$CI_RUN_DIR/baseline.peer.keys.sorted"
    cmp "$CI_RUN_DIR/baseline.expected.peer.keys.sorted" "$CI_RUN_DIR/baseline.peer.keys.sorted" || {
        echo "incomplete baseline compiler-speed peer coverage: peer cells do not exactly match the Jet matrix" >&2
        exit 1
    }
}


phase_value() {
    phase_text=$1
    phase_name=$2
    printf '%s\n' "$phase_text" | sed -n "s/.*;$phase_name=\\([^;]*\\).*/\\1/p"
}
current_row_field() {
    current_field_program=$1
    current_field_state=$2
    current_field_number=$3
    awk -F "$TAB" -v program="$current_field_program" -v state="$current_field_state" -v field="$current_field_number" \
        '$1 == program && $2 == state { print $field; exit }' "$CURRENT_ROWS"
}

FAIL=0
ROW_COUNT=0
CURRENT_ROWS="$CI_RUN_DIR/current.rows"
CURRENT_KEYS="$CI_RUN_DIR/current.keys"
: > "$CURRENT_KEYS"
CURRENT_PEER_ROWS="$CI_RUN_DIR/current.peer.rows"
printf '%s\n' "$CURRENT" | awk -v peer_header="$PEER_ROW_HEADER" '
    NR <= 2 { next }
    /^compiler-speed-peer version=/ { peer_section = 1; next }
    $0 == peer_header { peer_section = 1; next }
    peer_section { next }
    { print }
' > "$CURRENT_ROWS"
printf '%s\n' "$CURRENT" | awk -v peer_header="$PEER_ROW_HEADER" '
    /^compiler-speed-peer version=/ { peer_section = 1; next }
    $0 == peer_header { peer_section = 1; next }
    peer_section { print }
' > "$CURRENT_PEER_ROWS"
BASELINE_PEER_ROWS="$CI_RUN_DIR/baseline.peer.rows"
sed 's/.*"peers":\[//; s/\].*//' "$BASELINE" \
    | sed 's/},{/}\n{/g' > "$BASELINE_PEER_ROWS"
awk -F "$TAB" 'NF != 8 { exit 1 }' "$CURRENT_ROWS" || {
    echo "invalid compiler-speed row format" >&2
    exit 1
}
awk -F "$TAB" 'NF != 11 { exit 1 }' "$CURRENT_PEER_ROWS" || {
    echo "invalid compiler-speed peer row format" >&2
    exit 1
}
expected_rows=$((current_corpus_count * STATE_COUNT))
baseline_row_count=$(sed 's/],"peers":\[.*//' "$BASELINE" | sed 's/},{/}\n{/g' \
    | awk '/"program":"[^"]*","state":"[^"]*"/ { count++ } END { print count + 0 }')
[ "$baseline_row_count" -eq "$expected_rows" ] || {
    echo "baseline row count changed: expected $expected_rows, got $baseline_row_count" >&2
    exit 1
}
if [ "$current_peer_pending" -eq 1 ]; then
    expected_peer_rows=0
else
    expected_peer_rows=$((current_corpus_count * STATE_COUNT * current_peer_count * 2))
fi
[ "$current_peer_rows" -eq "$expected_peer_rows" ] || {
    echo "peer row count changed: expected $expected_peer_rows, got $current_peer_rows" >&2
    exit 1
}
if [ "$baseline_peer_pending" -eq 1 ]; then
    baseline_expected_peer_rows=0
else
    baseline_expected_peer_rows=$((expected_rows * baseline_peer_count * 2))
fi
[ "$baseline_peer_rows" -eq "$baseline_expected_peer_rows" ] || {
    echo "baseline peer row count changed: expected $baseline_expected_peer_rows, got $baseline_peer_rows" >&2
    exit 1
}
check_baseline_peer_rows
actual_peer_rows=$(wc -l < "$CURRENT_PEER_ROWS" | tr -d '[:space:]')
[ "$actual_peer_rows" -eq "$expected_peer_rows" ] || {
    echo "checked compiler-speed peer row count changed: expected $expected_peer_rows, got $actual_peer_rows" >&2
    exit 1
}

# Rows use the canonical tab-separated fields named by ROW_HEADER.
while IFS="$TAB" read -r row_program row_state row_stage row_latency row_memory row_variance row_output row_phases; do
    [ -n "${row_program:-}" ] || continue
    ROW_COUNT=$((ROW_COUNT + 1))
    is_positive_integer "$row_latency" || {
        echo "invalid current latency cell: $row_program/$row_state" >&2
        exit 1
    }
    is_positive_integer "$row_memory" || {
        echo "invalid current RSS cell: $row_program/$row_state" >&2
        exit 1
    }
    is_nonnegative_integer "$row_variance" || {
        echo "invalid current variance cell: $row_program/$row_state" >&2
        exit 1
    }
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
    [ -n "$base_stage" ] && [ -n "$base_phases" ] || {
        echo "baseline missing row: $row_program/$row_state" >&2
        exit 1
    }
    is_positive_integer "$base_latency" || {
        echo "invalid baseline latency cell: $row_program/$row_state" >&2
        exit 1
    }
    is_positive_integer "$base_memory" || {
        echo "invalid baseline RSS cell: $row_program/$row_state" >&2
        exit 1
    }
    is_nonnegative_integer "$base_variance" || {
        echo "invalid baseline variance cell: $row_program/$row_state" >&2
        exit 1
    }
    [ "$row_stage" = "$base_stage" ] || { echo "stage changed for $row_program/$row_state" >&2; exit 1; }
    row_stdout=${row_output%%:*}
    row_stderr=${row_output#*:}
    is_sha256 "$row_stdout" && is_sha256 "$row_stderr" || {
        echo "invalid current output identity: $row_program/$row_state" >&2
        exit 1
    }
    is_sha256 "$base_stdout" && is_sha256 "$base_stderr" || {
        echo "invalid baseline output identity: $row_program/$row_state" >&2
        exit 1
    }
    for metric in "latency_ns:$row_latency:$base_latency:$latency_threshold" "memory_bytes:$row_memory:$base_memory:$memory_threshold"; do
        metric_name=${metric%%:*}
        metric_rest=${metric#*:}
        metric_current=${metric_rest%%:*}
        metric_rest=${metric_rest#*:}
        metric_base=${metric_rest%%:*}
        metric_limit=${metric_rest#*:}
        if ! awk -v current="$metric_current" -v baseline="$metric_base" -v limit="$metric_limit" \
            'BEGIN { exit !(current <= baseline * (100 + limit) / 100) }'; then
            echo "REGRESSION $row_program/$row_state $metric_name: $metric_base -> $metric_current (threshold ${metric_limit}%)" >&2
            FAIL=1
        fi
    done
    if [ "$row_variance" -gt "$variance_budget" ]; then
        echo "UNSTABLE $row_program/$row_state interquartile spread=${row_variance}% budget=${variance_budget}%" >&2
        FAIL=1
    fi
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
            continue
        fi
        case "$identity_field" in
            workload_sha256|source_sha256|expected_sha256|manifest_sha256|libc_sha256|allocator_sha256|allocator_environment_sha256|hardware_sha256|topology_sha256|toolchain_sha256|rustc_sha256)
                is_sha256 "$current_identity" && is_sha256 "$base_identity" || {
                    echo "invalid workload/environment identity $identity_field for $row_program/$row_state" >&2
                    FAIL=1
                }
                ;;
            source_bytes|expected_bytes|cache_hits|cache_misses|artifact_bytes)
                is_nonnegative_integer "$current_identity" && is_nonnegative_integer "$base_identity" || {
                    echo "invalid numeric identity $identity_field for $row_program/$row_state" >&2
                    FAIL=1
                }
                ;;
            linker_sha256|linker_backend_sha256)
                case "$current_identity:$base_identity" in
                    none:none) ;;
                    *) is_sha256 "$current_identity" && is_sha256 "$base_identity" || {
                        echo "invalid linker identity $identity_field for $row_program/$row_state" >&2
                        FAIL=1
                    } ;;
                esac
                ;;
            *)
                case "$current_identity:$base_identity" in
                    *null*|*missing*|*invalid*|*inconclusive*)
                        echo "inconclusive identity $identity_field for $row_program/$row_state" >&2
                        FAIL=1
                        ;;
                esac
                ;;
        esac
        if [ "$current_identity" != "$base_identity" ]; then
            echo "unmatched workload/environment identity $identity_field for $row_program/$row_state" >&2
            FAIL=1
        fi
    done
done < "$CURRENT_ROWS"

if [ "$current_peer_pending" -eq 0 ]; then

PEER_KEYS_FILE="$CI_RUN_DIR/current.peer.keys"
PEER_TOOLCHAINS_FILE="$CI_RUN_DIR/current.peer.toolchains"
PEER_ROW_COUNT=0
: > "$PEER_KEYS_FILE"
: > "$PEER_TOOLCHAINS_FILE"
while IFS="$TAB" read -r peer_name peer_language peer_program peer_state peer_metric peer_value \
    peer_workload peer_source peer_expected peer_manifest_row peer_toolchain; do
    [ -n "${peer_name:-}" ] || continue
    PEER_ROW_COUNT=$((PEER_ROW_COUNT + 1))
    if ! printf '%s\n' "$current_peer_keys" | awk -F, -v wanted="$peer_name:$peer_language" '
        { for (i = 1; i <= NF; i++) if ($i == wanted) found = 1 }
        END { exit !found }
    '; then
        echo "compiler-speed peer row names undeclared peer: $peer_name/$peer_language" >&2
        FAIL=1
        continue
    fi
    peer_row_key=$(printf '%s\t%s\t%s\t%s\t%s' "$peer_name" "$peer_language" "$peer_program" "$peer_state" "$peer_metric")
    if grep -Fqx -- "$peer_row_key" "$PEER_KEYS_FILE"; then
        echo "duplicate compiler-speed peer row: $peer_row_key" >&2
        FAIL=1
        continue
    fi
    printf '%s\n' "$peer_row_key" >> "$PEER_KEYS_FILE"
    case "$peer_metric" in
        latency_ns) jet_value=$(current_row_field "$peer_program" "$peer_state" 4) ;;
        memory_bytes) jet_value=$(current_row_field "$peer_program" "$peer_state" 5) ;;
        *) echo "invalid compiler-speed peer metric: $peer_metric" >&2; FAIL=1; continue ;;
    esac
    peer_phases=$(current_row_field "$peer_program" "$peer_state" 8)
    if [ -z "$peer_phases" ] || [ -z "$jet_value" ]; then
        echo "compiler-speed peer row names unknown Jet row: $peer_program/$peer_state" >&2
        FAIL=1
        continue
    fi
    is_positive_integer "$jet_value" || {
        echo "invalid Jet compiler-speed cell: $peer_program/$peer_state/$peer_metric" >&2
        FAIL=1
        continue
    }
    is_positive_integer "$peer_value" || {
        echo "invalid compiler-speed peer value: $peer_name/$peer_program/$peer_state/$peer_metric" >&2
        FAIL=1
        continue
    }
    for peer_hash in "$peer_workload" "$peer_source" "$peer_expected" "$peer_manifest_row" "$peer_toolchain"; do
        is_sha256 "$peer_hash" || {
            echo "invalid compiler-speed peer input identity: $peer_name/$peer_program/$peer_state" >&2
            FAIL=1
        }
    done
    jet_workload=$(phase_value "$peer_phases" workload_sha256)
    jet_source=$(phase_value "$peer_phases" source_sha256)
    jet_expected=$(phase_value "$peer_phases" expected_sha256)
    if [ "$peer_workload" != "$jet_workload" ]; then
        echo "compiler-speed peer workload identity mismatch: $peer_name/$peer_program/$peer_state" >&2
        FAIL=1
    fi
    if [ "$peer_source" != "$jet_source" ] || [ "$peer_expected" != "$jet_expected" ]; then
        echo "compiler-speed peer input identity mismatch: $peer_name/$peer_program/$peer_state" >&2
        FAIL=1
    fi
    [ "$peer_manifest_row" = "$current_manifest" ] || {
        echo "compiler-speed peer manifest mismatch: $peer_name/$peer_program/$peer_state" >&2
        FAIL=1
    }
    baseline_peer_workload=$(baseline_peer_field "$peer_name" "$peer_language" "$peer_program" "$peer_state" "$peer_metric" workload_sha256)
    baseline_peer_source=$(baseline_peer_field "$peer_name" "$peer_language" "$peer_program" "$peer_state" "$peer_metric" source_sha256)
    baseline_peer_expected=$(baseline_peer_field "$peer_name" "$peer_language" "$peer_program" "$peer_state" "$peer_metric" expected_sha256)
    baseline_peer_manifest_row=$(baseline_peer_field "$peer_name" "$peer_language" "$peer_program" "$peer_state" "$peer_metric" manifest_sha256)
    baseline_peer_toolchain=$(baseline_peer_field "$peer_name" "$peer_language" "$peer_program" "$peer_state" "$peer_metric" toolchain_sha256)
    for peer_identity in \
        "workload_sha256:$peer_workload:$baseline_peer_workload" \
        "source_sha256:$peer_source:$baseline_peer_source" \
        "expected_sha256:$peer_expected:$baseline_peer_expected" \
        "manifest_sha256:$peer_manifest_row:$baseline_peer_manifest_row" \
        "toolchain_sha256:$peer_toolchain:$baseline_peer_toolchain"; do
        peer_identity_name=${peer_identity%%:*}
        peer_identity_rest=${peer_identity#*:}
        peer_identity_current=${peer_identity_rest%%:*}
        peer_identity_baseline=${peer_identity_rest#*:}
        [ -n "$peer_identity_baseline" ] || {
            echo "baseline missing compiler-speed peer identity $peer_identity_name: $peer_name/$peer_program/$peer_state/$peer_metric" >&2
            FAIL=1
            continue
        }
        [ "$peer_identity_current" = "$peer_identity_baseline" ] || {
            echo "compiler-speed peer identity changed $peer_identity_name: $peer_name/$peer_program/$peer_state/$peer_metric" >&2
            FAIL=1
        }
    done
    printf '%s:%s\t%s\n' "$peer_name" "$peer_language" "$peer_toolchain" >> "$PEER_TOOLCHAINS_FILE"
    if [ "$peer_language" = rust ]; then
        if ! awk -v jet="$jet_value" -v peer="$peer_value" 'BEGIN { exit !(jet / peer <= 1.05) }'; then
            echo "compiler-speed peer loss: $peer_name/$peer_program/$peer_state/$peer_metric Jet=$jet_value peer=$peer_value ratio>1.05" >&2
            FAIL=1
        fi
    elif ! awk -v jet="$jet_value" -v peer="$peer_value" 'BEGIN { exit !(jet / peer < 1.00) }'; then
        echo "compiler-speed peer loss: $peer_name/$peer_program/$peer_state/$peer_metric Jet=$jet_value peer=$peer_value ratio>=1.00" >&2
        FAIL=1
    fi
done < "$CURRENT_PEER_ROWS"

awk -F "$TAB" '
    {
        if (toolchain[$1] == "") toolchain[$1] = $2
        else if (toolchain[$1] != $2) invalid = 1
    }
    END { exit invalid }
' "$PEER_TOOLCHAINS_FILE" || {
    echo "compiler-speed peer toolchain identity changed within a peer" >&2
    FAIL=1
}

expected_peer_keys="$CI_RUN_DIR/expected.peer.keys"
awk -F "$TAB" -v declarations="$current_peer_keys" '
    BEGIN {
        peer_count = split(declarations, peers, ",")
        metric_count = split("latency_ns,memory_bytes", metrics, ",")
    }
    {
        row_key = $1 SUBSEP $2
        if (seen[row_key]++) invalid = 1
        for (peer_index = 1; peer_index <= peer_count; peer_index++) {
            split(peers[peer_index], pair, ":")
            for (metric_index = 1; metric_index <= metric_count; metric_index++) {
                printf "%s\t%s\t%s\t%s\t%s\n", pair[1], pair[2], $1, $2, metrics[metric_index]
            }
        }
    }
    END {
        if (invalid || NR == 0) exit 1
    }
' "$CURRENT_ROWS" > "$expected_peer_keys" || {
    echo "duplicate or missing Jet compiler-speed rows" >&2
    exit 1
}
sort "$expected_peer_keys" > "$CI_RUN_DIR/expected.peer.keys.sorted"
sort "$PEER_KEYS_FILE" > "$CI_RUN_DIR/current.peer.keys.sorted"
cmp "$CI_RUN_DIR/expected.peer.keys.sorted" "$CI_RUN_DIR/current.peer.keys.sorted" || {
    echo "incomplete compiler-speed peer coverage: peer cells do not exactly match the Jet matrix" >&2
    exit 1
}

[ "$PEER_ROW_COUNT" -eq "$expected_peer_rows" ] || {
    echo "checked compiler-speed peer row count changed: expected $expected_peer_rows, got $PEER_ROW_COUNT" >&2
    exit 1
}
fi

[ "$ROW_COUNT" -eq "$expected_rows" ] || {
    echo "checked corpus row count changed: expected $expected_rows, got $ROW_COUNT" >&2
    exit 1
}
[ "$FAIL" -eq 0 ] || { echo "perf gate FAILED" >&2; exit 1; }
if [ "$current_peer_pending" -eq 1 ]; then
    echo "perf gate OK (candidate ${candidate_commit}, latency ${latency_threshold}%, memory ${memory_threshold}%, variance ${variance_budget}%, rows ${ROW_COUNT}, peer gate pending D-BUILDBENCH1)"
else
    echo "perf gate OK (candidate ${candidate_commit}, latency ${latency_threshold}%, memory ${memory_threshold}%, variance ${variance_budget}%, rows ${ROW_COUNT})"
fi
