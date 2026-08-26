#!/usr/bin/env bash
# Card #1909: fail CI when a checked web workload grows beyond its baseline.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASELINE="$ROOT/tools/perf/web-bundle-baseline.tsv"
JET_ENV="$ROOT/scripts/agent/jet-env"
SCRATCH_ROOT="${TMPDIR:-$HOME/.cache/jet-test-scratch}"

fail() {
  echo "web bundle gate: $*" >&2
  exit 1
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

file_bytes() {
  wc -c < "$1" | tr -d '[:space:]'
}

check_size() {
  local workload="$1" metric="$2" baseline="$3" current="$4" percent="$5"
  local limit=$((baseline + (baseline * percent + 99) / 100))
  if (( current > limit )); then
    echo "REGRESSION $workload $metric: $baseline -> $current (limit $limit, +${percent}%)" >&2
    return 1
  fi
}

[[ -s "$BASELINE" ]] || fail "missing baseline: ${BASELINE#$ROOT/}"
[[ -x "$JET_ENV" ]] || fail "missing Jet environment: ${JET_ENV#$ROOT/}"
expected_header=$'version\tworkload\tsource\tsource_sha256\twasm_bytes\tglue_bytes\truntime_bytes\tbundle_bytes\tregression_pct'
[[ "$(sed -n '2p' "$BASELINE")" == "$expected_header" ]] || fail "baseline schema drifted"
mkdir -p "$SCRATCH_ROOT"
run_dir="$(mktemp -d "$SCRATCH_ROOT/web-bundle.XXXXXX")"
trap 'rm -rf "$run_dir"' EXIT HUP INT TERM

failures=0
rows=0
declare -A seen_workloads=()
while IFS=$'\t' read -r version workload source source_hash wasm_base glue_base runtime_base bundle_base regression; do
  [[ -z "${version:-}" || "$version" == \#* ]] && continue
  [[ "$version" == "version" ]] && continue
  [[ "$version" == "1" ]] || fail "unsupported baseline row version: ${version:-missing}"
  [[ "$workload" =~ ^[a-z][a-z0-9-]*$ ]] || fail "invalid workload: $workload"
  [[ -z "${seen_workloads[$workload]+x}" ]] || fail "duplicate workload: $workload"
  seen_workloads[$workload]=1
  [[ "$source" != /* && "$source" != *..* ]] || fail "unsafe workload source: $source"
  source_path="$ROOT/$source"
  [[ -f "$source_path" ]] || fail "missing workload source: $source"
  [[ "$source_hash" =~ ^[0-9a-f]{64}$ ]] || fail "invalid source hash: $source"
  [[ "$source_hash" == "$(sha256_file "$source_path")" ]] || fail "source changed: $source"
  for value in "$wasm_base" "$glue_base" "$runtime_base" "$bundle_base" "$regression"; do
    [[ "$value" =~ ^[0-9]+$ ]] || fail "invalid baseline value for $workload"
  done
  (( wasm_base > 0 && glue_base > 0 && runtime_base > 0 && bundle_base == wasm_base + glue_base + runtime_base )) || fail "invalid bundle totals: $workload"
  (( regression <= 100 )) || fail "regression limit exceeds 100%: $workload"

  case_dir="$run_dir/$workload"
  mkdir -p "$case_dir"
  cp "$source_path" "$case_dir/main.jet"
  (
    cd "$case_dir"
    JET_ROOT="$ROOT" TMPDIR="$SCRATCH_ROOT" "$JET_ENV" jet build --target=web main.jet
  ) >"$case_dir/build.log" 2>&1 || {
    sed -n '1,120p' "$case_dir/build.log" >&2
    fail "build failed: $workload"
  }
  for artifact in app.wasm app.js jet_dom_runtime.js; do
    artifact_path="$case_dir/build/$artifact"
    [[ -f "$artifact_path" && ! -L "$artifact_path" ]] || fail "missing regular artifact: $workload/$artifact"
  done

  wasm_current="$(file_bytes "$case_dir/build/app.wasm")"
  glue_current="$(file_bytes "$case_dir/build/app.js")"
  runtime_current="$(file_bytes "$case_dir/build/jet_dom_runtime.js")"
  bundle_current=$((wasm_current + glue_current + runtime_current))
  printf '%s\twasm=%s\tglue=%s\truntime=%s\tbundle=%s\n' \
    "$workload" "$wasm_current" "$glue_current" "$runtime_current" "$bundle_current"

  check_size "$workload" wasm "$wasm_base" "$wasm_current" "$regression" || failures=$((failures + 1))
  check_size "$workload" glue "$glue_base" "$glue_current" "$regression" || failures=$((failures + 1))
  check_size "$workload" runtime "$runtime_base" "$runtime_current" "$regression" || failures=$((failures + 1))
  check_size "$workload" bundle "$bundle_base" "$bundle_current" "$regression" || failures=$((failures + 1))
  rows=$((rows + 1))
done < "$BASELINE"

[[ "$rows" -eq 3 ]] || fail "expected 3 web workloads, got $rows"
[[ "$failures" -eq 0 ]] || fail "$failures size regression(s)"
echo "web bundle gate: pass workloads=$rows"
