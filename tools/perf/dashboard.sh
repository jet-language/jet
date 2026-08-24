#!/usr/bin/env sh
# Compiler-speed corpus dashboard.
#
# The corpus, source/golden hashes, optimized AOT stage, timing phases, and
# machine identity are one report. A changed workload is an error, not a new
# baseline. Refresh tools/perf/baseline.json only after reviewing the result.
#
# Usage:
#   tools/perf/dashboard.sh                    # measure the checked corpus
#   tools/perf/dashboard.sh --baseline         # measure and write the baseline
#   tools/perf/dashboard.sh --compare FILE     # measure and print FILE

set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PERF_DIR="$ROOT/tools/perf"
CORPUS="$PERF_DIR/corpus.tsv"
BASELINE="$PERF_DIR/baseline.json"
TMP_ROOT=${TMPDIR:-"$HOME/.cache/jet-test-scratch"}

if [ ! -d "$TMP_ROOT" ]; then
    mkdir -p "$TMP_ROOT"
fi

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

require_relative_file() {
    case "$1" in
        ""|/*|*".."*)
            echo "invalid corpus path: $1" >&2
            exit 1
            ;;
    esac
    if [ ! -f "$ROOT/$1" ]; then
        echo "missing corpus file: $1" >&2
        exit 1
    fi
}

machine_os=$(uname -s)
machine_arch=$(uname -m)
machine_cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo unknown)
machine_host=$(hostname 2>/dev/null || echo unknown)
case "$machine_cpus" in
    ''|*[!0-9]*) echo "unavailable machine CPU identity: $machine_cpus" >&2; exit 1 ;;
esac
case "$machine_host" in
    ''|*'"'*) echo "unavailable machine host identity" >&2; exit 1 ;;
esac
machine="$machine_os/$machine_arch/cpus=$machine_cpus/host=$machine_host"
stage="aot-release"

corpus_sha=$(sha256 "$CORPUS")
rows_file="$TMP_ROOT/compiler-speed-rows.$$.tsv"
run_dir="$TMP_ROOT/compiler-speed.$$.d"
mkdir -p "$run_dir"
trap 'rm -rf "$run_dir" "$rows_file" "$ROOT/jet-timing.json" "$ROOT/build/jet-timing-backend.json"' EXIT HUP INT TERM

check_corpus() {
    count=0
    while IFS="	" read -r program expected source_hash expected_hash; do
        case "$program" in
            ""|\#*) continue ;;
        esac
        if [ -z "${expected:-}" ] || [ -z "${source_hash:-}" ] || [ -z "${expected_hash:-}" ]; then
            echo "malformed corpus row: $program" >&2
            exit 1
        fi
        require_relative_file "$program"
        require_relative_file "$expected"
        actual=$(sha256 "$ROOT/$program")
        [ "$actual" = "$source_hash" ] || {
            echo "corpus source changed: $program ($source_hash -> $actual)" >&2
            exit 1
        }
        actual=$(sha256 "$ROOT/$expected")
        [ "$actual" = "$expected_hash" ] || {
            echo "corpus golden changed: $expected ($expected_hash -> $actual)" >&2
            exit 1
        }
        count=$((count + 1))
    done < "$CORPUS"
    [ "$count" -gt 0 ] || { echo "empty compiler-speed corpus" >&2; exit 1; }
    echo "$count"
}

field() {
    file=$1
    name=$2
    sed 's/},{/}\n{/g' "$file" \
        | grep '"name":"'"$name"'"' \
        | sed 's/.*"us"://; s/[^0-9].*//' \
        | head -n1
}

measure_one() {
    program=$1
    expected=$2
    sample="$run_dir/$(echo "$program" | tr '/.' '__')"
    mkdir -p "$sample/build"
    log="$sample/command.log"
    timing="$sample/jet-timing.json"
    backend="$sample/build/jet-timing-backend.json"
    rm -f "$ROOT/jet-timing.json" "$ROOT/build/jet-timing-backend.json"

    if ! (cd "$ROOT" && \
        JET_TIMING=1 JET_TIMING_DIR="$sample" JET_CACHE_DIR="$sample/cache" \
        "$ROOT/scripts/agent/jet-env" jet build --release "$program" >"$log" 2>&1); then
        echo "compiler-speed build failed: $program" >&2
        sed -n '1,120p' "$log" >&2
        exit 1
    fi
    [ -s "$timing" ] || { echo "missing timing report: $program" >&2; exit 1; }
    [ -s "$backend" ] || { echo "missing backend timing report: $program" >&2; exit 1; }

    load=$(field "$timing" load)
    sema=$(field "$timing" sema)
    ffi=$(field "$timing" ffi)
    codegen=$(field "$timing" codegen)
    rust_bytes=$(field "$timing" rust_bytes)
    backend_link=$(field "$backend" backend_link)
    binary_bytes=$(grep 'jet-timing binary_bytes=' "$log" | sed 's/.*binary_bytes=//' | head -n1)
    for value in "$load" "$sema" "$ffi" "$codegen" "$rust_bytes" "$backend_link" "$binary_bytes"; do
        case "$value" in
            ""|*[!0-9]*) echo "incomplete timing report: $program" >&2; exit 1 ;;
        esac
    done
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$program" "$stage" "$load" "$sema" "$ffi" "$codegen" \
        "$backend_link" "$rust_bytes" "$binary_bytes" >> "$rows_file"

    # The optimized AOT artifact must retain the checked golden output.
    binary="$ROOT/build/$(basename "$program" .jet)"
    [ -x "$binary" ] || { echo "missing AOT artifact: $binary" >&2; exit 1; }
    actual="$sample/actual.out"
    "$binary" >"$actual"
    cmp "$ROOT/$expected" "$actual" || {
        echo "AOT output mismatch: $program" >&2
        diff -u "$ROOT/$expected" "$actual" >&2 || true
        exit 1
    }
}

count=$(check_corpus)
while IFS="	" read -r program expected source_hash expected_hash; do
    case "$program" in
        ""|\#*) continue ;;
    esac
    measure_one "$program" "$expected"
done < "$CORPUS"

print_table() {
    printf '%-48s %12s %8s %8s %8s %8s %12s %10s %12s\n' \
        program stage load_us sema_us ffi_us codegen_us backend_us rust_B binary_B
    while IFS="	" read -r program run_stage load sema ffi codegen backend rust_bytes binary_bytes; do
        printf '%-48s %12s %8s %8s %8s %8s %12s %10s %12s\n' \
            "$program" "$run_stage" "$load" "$sema" "$ffi" "$codegen" \
            "$backend" "$rust_bytes" "$binary_bytes"
    done < "$rows_file"
}

as_json() {
    printf '{"schema":"jet.compiler-speed","version":1,"corpus_sha256":"%s",' "$corpus_sha"
    printf '"stage":"%s","machine":{"os":"%s","arch":"%s","cpus":%s,"hostname":"%s"},"runs":[' \
        "$stage" "$machine_os" "$machine_arch" "$machine_cpus" "$machine_host"
    first=1
    while IFS="	" read -r program run_stage load sema ffi codegen backend rust_bytes binary_bytes; do
        [ "$first" -eq 1 ] || printf ','
        first=0
        printf '{"program":"%s","stage":"%s","load":%s,"sema":%s,"ffi":%s,"codegen":%s,"backend_link":%s,"rust_bytes":%s,"binary_bytes":%s}' \
            "$program" "$run_stage" "$load" "$sema" "$ffi" "$codegen" "$backend" "$rust_bytes" "$binary_bytes"
    done < "$rows_file"
    printf ']}\n'
}

echo "compiler-speed corpus=$count corpus_sha256=$corpus_sha stage=$stage machine=$machine"
print_table

case "${1:-}" in
    "") ;;
    --baseline)
        as_json > "$BASELINE"
        echo "baseline written to $BASELINE"
        ;;
    --compare)
        file=${2:-$BASELINE}
        [ -f "$file" ] || { echo "missing baseline: $file" >&2; exit 1; }
        echo "baseline=$file"
        sed -n '1,80p' "$file"
        ;;
    *)
        echo "usage: tools/perf/dashboard.sh [--baseline|--compare FILE]" >&2
        exit 2
        ;;
esac
