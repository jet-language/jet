#!/usr/bin/env bash
# c730: strict JIT↔AOT differential parity gate (#727 corpus owner).
# Runs default tiered + optimized AOT identity per case. It compares the pure
# interpreter when its boundary allows execution. Weighted corpus shards compose
# their reports, then run the whole-corpus compile and run parity audit.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

REPORT_DIR="${JET_CORPUS_GATE_REPORT_DIR:-$ROOT/jit-aot-parity-report}"
mkdir -p "$REPORT_DIR"
# Twelve weighted slices leave headroom for per-stem variance while keeping the
# 900s suite budget unchanged and the covered set complete.
SHARD_COUNT="${JET_CORPUS_GATE_SHARD_COUNT:-12}"
case "$SHARD_COUNT" in
  ''|*[!0-9]*) echo "error: JET_CORPUS_GATE_SHARD_COUNT must be a positive integer" >&2; exit 64 ;;
esac
if [ "$SHARD_COUNT" -lt 1 ]; then
  echo "error: JET_CORPUS_GATE_SHARD_COUNT must be >= 1" >&2
  exit 64
fi
export JET_CORPUS_GATE_REPORT_DIR="$REPORT_DIR"
export JET_WRITE_CORPUS_GATE="${JET_WRITE_CORPUS_GATE:-0}"
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
{
  echo "# Target-only exclusions leave the parity matrix when Jet does not"
  echo "# support native Cranelift JIT on that host (cranelift_host_supported)."
  echo "aarch64-*-*: cranelift-jit host path unsupported (x86_64 only today)"
  echo "macos-arm64 / macos-latest (Apple Silicon): not a supported native JIT host"
  echo "windows-arm64: not a supported native JIT host"
} >"$REPORT_DIR/target_exclusions.txt"

# Placeholder until all shards and composition succeed; failures keep this + gate.log.
echo "failed" >"$REPORT_DIR/result.txt"
: >"$REPORT_DIR/output_diff.txt"
: >"$REPORT_DIR/gate.log"

set +e
START_NS="$(date +%s%N 2>/dev/null || echo 0)"
GATE_STATUS=0
for ((shard = 0; shard < SHARD_COUNT; shard++)); do
  shard_dir="$REPORT_DIR/shard-$shard"
  mkdir -p "$shard_dir"
  shard_started="$(date +%s%N 2>/dev/null || echo 0)"
  JET_CORPUS_GATE_SHARD_INDEX="$shard" \
  JET_CORPUS_GATE_SHARD_COUNT="$SHARD_COUNT" \
  JET_DUMP_CORPUS_GATE=1 \
  JET_CORPUS_GATE_REPORT_DIR="$shard_dir" \
    cargo test --test dev_corpus_gate example_corpus_strict_jit_aot_differential_gate -- --exact --nocapture \
      2>&1 | tee "$shard_dir/gate.log" | tee -a "$REPORT_DIR/gate.log"
  shard_status=${PIPESTATUS[0]}
  if [[ "$shard_status" -ne 0 ]]; then GATE_STATUS=1; fi
  shard_finished="$(date +%s%N 2>/dev/null || echo 0)"
  if [[ "$shard_started" != "0" && "$shard_finished" != "0" ]]; then
    # Keep wrapper timing separate: the Rust report owns timing.txt, and an
    # append would make a rerun's first stale wrapper_elapsed_ms win below.
    printf 'wrapper_elapsed_ms=%s\n' "$(( (shard_finished - shard_started) / 1000000 ))" \
      >"$shard_dir/wrapper_timing.txt"
  fi
done

{
  for ((shard = 0; shard < SHARD_COUNT; shard++)); do
    timing_file="$REPORT_DIR/shard-$shard/wrapper_timing.txt"
    elapsed_ms="$(awk -F= '$1 == "wrapper_elapsed_ms" { print $2; exit }' "$timing_file" 2>/dev/null || true)"
    if [[ -z "$elapsed_ms" ]]; then
      elapsed_ms="unknown"
    fi
    if [[ "$elapsed_ms" =~ ^[0-9]+$ ]]; then
      printf 'shard=%s elapsed_ms=%s elapsed_s=%.3f\n' "$shard" "$elapsed_ms" "$(awk -v ms="$elapsed_ms" 'BEGIN { printf "%.3f", ms / 1000 }')"
    else
      printf 'shard=%s elapsed_ms=%s\n' "$shard" "$elapsed_ms"
    fi
  done
} >"$REPORT_DIR/shard_timings.txt"
cat "$REPORT_DIR/shard_timings.txt"

COMPOSE_STATUS=1
if [[ "$GATE_STATUS" -eq 0 ]]; then
  bash tools/ci/compose-corpus-gate.sh "$REPORT_DIR" "$SHARD_COUNT" \
    2>&1 | tee -a "$REPORT_DIR/gate.log"
  COMPOSE_STATUS=${PIPESTATUS[0]}
  if [[ "$COMPOSE_STATUS" -ne 0 ]]; then GATE_STATUS=1; fi
fi

cargo test --test dev_corpus jit_coverage_audit -- --exact --nocapture \
  2>&1 | tee -a "$REPORT_DIR/gate.log"
RATCHET_STATUS=${PIPESTATUS[0]}
END_NS="$(date +%s%N 2>/dev/null || echo 0)"
set -e

if [[ "$START_NS" != "0" && "$END_NS" != "0" ]]; then
  ELAPSED_MS=$(( (END_NS - START_NS) / 1000000 ))
  printf 'shard_count=%s\nwrapper_elapsed_ms=%s\n' "$SHARD_COUNT" "$ELAPSED_MS" >"$REPORT_DIR/timing.txt"
fi

STATUS=0
if [[ "$GATE_STATUS" -ne 0 || "$COMPOSE_STATUS" -ne 0 || "$RATCHET_STATUS" -ne 0 ]]; then
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

echo "jit-aot-parity: gate=$GATE_STATUS compose=$COMPOSE_STATUS ratchet=$RATCHET_STATUS shards=$SHARD_COUNT report=$REPORT_DIR"
exit "$STATUS"
