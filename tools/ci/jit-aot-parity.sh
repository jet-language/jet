#!/usr/bin/env bash
# c730: strict JIT↔AOT differential parity gate (#727 corpus owner).
# Runs pure-interpreter + default tiered + optimized AOT identity per case,
# plus the shrink-only jit_gaps.txt compile ratchet.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

REPORT_DIR="${JET_CORPUS_GATE_REPORT_DIR:-$ROOT/jit-aot-parity-report}"
mkdir -p "$REPORT_DIR"
export JET_CORPUS_GATE_REPORT_DIR="$REPORT_DIR"
export JET_REQUIRE_CRANELIFT_HOST="${JET_REQUIRE_CRANELIFT_HOST:-1}"
export JET_REQUIRE_RUSTC="${JET_REQUIRE_RUSTC:-1}"
# collect_jit_coverage + corpus classification need a deep stack on some hosts.
export RUST_MIN_STACK="${RUST_MIN_STACK:-16777216}"

{
  echo "commit=$(git rev-parse HEAD)"
  echo "commit_short=$(git rev-parse --short HEAD)"
  echo "ref=${GITHUB_REF:-$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)}"
  if command -v rustc >/dev/null 2>&1; then
    echo "target=$(rustc -vV | awk '/^host:/{print $2}')"
  else
    echo "target=unknown"
  fi
  echo "os=$(uname -s 2>/dev/null || echo unknown)"
  echo "arch=$(uname -m 2>/dev/null || echo unknown)"
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || echo unknown)"
} >"$REPORT_DIR/commit.txt"

{
  echo "=== rustc ==="
  if command -v rustc >/dev/null 2>&1; then rustc -vV; else echo "(rustc not on PATH)"; fi
  echo
  echo "=== cargo ==="
  if command -v cargo >/dev/null 2>&1; then cargo -vV; else echo "(cargo not on PATH)"; fi
  echo
  echo "=== rustup show ==="
  if command -v rustup >/dev/null 2>&1; then rustup show; else echo "(rustup not on PATH)"; fi
} >"$REPORT_DIR/toolchain.txt"

cp -f tests/jit_corpus_gate.txt "$REPORT_DIR/case_list_manifest.txt"
cp -f tests/jit_gaps.txt "$REPORT_DIR/jit_gaps_manifest.txt"
{
  echo "# Target-only exclusions leave the parity matrix when Jet does not"
  echo "# support native Cranelift JIT on that host (cranelift_host_supported)."
  echo "aarch64-*-*: cranelift-jit host path unsupported (x86_64 only today)"
  echo "macos-arm64 / macos-latest (Apple Silicon): not a supported native JIT host"
  echo "windows-arm64: not a supported native JIT host"
} >"$REPORT_DIR/target_exclusions.txt"

# Placeholder until the gate overwrites on success; failures keep this + gate.log.
echo "failed" >"$REPORT_DIR/result.txt"
: >"$REPORT_DIR/output_diff.txt"

set +e
START_NS="$(date +%s%N 2>/dev/null || echo 0)"
cargo test --test dev example_corpus_strict_jit_aot_differential_gate -- --exact --nocapture \
  2>&1 | tee "$REPORT_DIR/gate.log"
GATE_STATUS=${PIPESTATUS[0]}
cargo test --test dev jit_try_compile_manifest_matches -- --exact --nocapture \
  2>&1 | tee -a "$REPORT_DIR/gate.log"
RATCHET_STATUS=${PIPESTATUS[0]}
END_NS="$(date +%s%N 2>/dev/null || echo 0)"
set -e

if [[ "$START_NS" != "0" && "$END_NS" != "0" ]]; then
  ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))
  echo "wrapper_elapsed_ms=$ELAPSED_MS" >>"$REPORT_DIR/timing.txt"
fi

STATUS=0
if [[ "$GATE_STATUS" -ne 0 || "$RATCHET_STATUS" -ne 0 ]]; then
  STATUS=1
  echo "failed" >"$REPORT_DIR/result.txt"
  # Surface assertion diffs for the artifact upload.
  if grep -nE 'left:|right:|assertion|panicked|drifted|must match' "$REPORT_DIR/gate.log" \
    >"$REPORT_DIR/output_diff.txt"; then
    :
  else
    echo "(no assert diff lines captured; see gate.log)" >"$REPORT_DIR/output_diff.txt"
  fi
else
  # Gate writes result.txt=ok when JET_CORPUS_GATE_REPORT_DIR is set.
  if [[ ! -f "$REPORT_DIR/result.txt" ]] || ! grep -qx 'ok' "$REPORT_DIR/result.txt"; then
    echo "ok" >"$REPORT_DIR/result.txt"
  fi
fi

echo "jit-aot-parity: gate=$GATE_STATUS ratchet=$RATCHET_STATUS report=$REPORT_DIR"
exit "$STATUS"
