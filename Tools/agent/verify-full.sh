#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Keep stdout reserved for probe data (for example, the four temp-root rows).
# Preflight status belongs on stderr so callers can parse stdout safely.
if [ "${JET_NIX_TMP_CLEANED:-}" != "1" ]; then
  "$repo/Tools/agent/clean-nix-tmp.sh"
fi
export JET_NIX_TMP_CLEANED=1
probe_mode=""
case "${1:-}" in
  --probe-temp-root)
    probe_mode="exit"
    shift
    ;;
  --probe-temp-root-signal)
    probe_mode="signal"
    shift
    ;;
esac
if [ -z "$probe_mode" ]; then
  # Full verification is an epoch/release closeout, never a per-card
  # confidence check. Bind it to a review-ready milestone and frozen HEAD
  # before any verification work starts.
  node "$repo/Tools/agent/closeout-gate.mjs" check
fi
node "$repo/Tools/agent/check-unsafe-ratchet.mjs" >&2
tmp_parent="${JET_VERIFY_TMPDIR:-${TMPDIR:-$HOME/.cache/jet-dev/scratch}}"
case "$tmp_parent" in
  /tmp|/tmp/*)
    echo "error: verify-full scratch must not use /tmp; use $HOME/.cache/jet-dev/scratch" >&2
    exit 1
    ;;
esac
mkdir -p -- "$tmp_parent"
tmp="$(mktemp -d "$tmp_parent/jet-verify.XXXXXX")"
oracle_cache_dir="$repo/.tmp/jet-test-scratch/verify-oracles-$$"
cleanup() {
  # card 1640: artifact footprint stays visible every run — target/ has no
  # automatic pruning and once reached 619G unnoticed.
  target_kb="$(du -sk -- "${CARGO_TARGET_DIR:-$repo/target}" 2>/dev/null | cut -f1 || true)"
  scratch_kb="$(du -sk -- "$tmp" 2>/dev/null | cut -f1 || true)"
  echo "artifact footprint: target=$(( ${target_kb:-0} / 1048576 ))G scratch=$(( ${scratch_kb:-0} / 1024 ))M" >&2
  if [ "${target_kb:-0}" -gt 157286400 ]; then
    echo "warning: target/ exceeds 150G — run cargo clean (see AGENTS.md pruning note)" >&2
  fi
  rm -rf -- "$tmp"
  rm -rf -- "$oracle_cache_dir"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

export TMPDIR="$tmp"
export TMP="$tmp"
export TEMP="$tmp"
export JET_VERIFY_TMPDIR="$tmp"
export JET_DEV_ORACLE_CACHE_DIR="$oracle_cache_dir"
export JET_TEST_JOBS="${JET_TEST_JOBS:-6}"
requested_build_jobs="${CARGO_BUILD_JOBS:-$JET_TEST_JOBS}"
case "$requested_build_jobs" in
  ''|*[!0-9]*)
    echo "error: CARGO_BUILD_JOBS must be a non-negative integer" >&2
    exit 1
    ;;
esac
if [ "$requested_build_jobs" -gt 6 ]; then
  requested_build_jobs=6
fi
export CARGO_BUILD_JOBS="$requested_build_jobs"

# D-CI2=A: every workspace test shard inherits the warning wall. The separate
# rust-lint job owns rustfmt and Clippy; this keeps the real test path strict.
case " ${RUSTFLAGS:-} " in
  *" -D warnings "*) ;;
  *) export RUSTFLAGS="${RUSTFLAGS:-} -D warnings" ;;
esac

export JET_CI_EVIDENCE_DIR="${JET_CI_EVIDENCE_DIR:-$repo/.tmp/ci-evidence/${JET_TEST_SHARD:-all}-$$}"
mkdir -p -- "$JET_CI_EVIDENCE_DIR"
export JET_TOWER_HYGIENE_REPORT="${JET_TOWER_HYGIENE_REPORT:-$JET_CI_EVIDENCE_DIR/tower-hygiene.txt}"

# Ordinary independent failures accumulate. Logs and the complete status table
# live outside the temporary tree removed by cleanup.
verify_status=0
verify_unsafe=""
verify_target="-"
verify_summary="$JET_CI_EVIDENCE_DIR/verify-full-summary.tsv"
printf 'status\tscope\trc\tprerequisite\tlog\ttarget\n' > "$verify_summary"
verify_record() {
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" "$verify_target" >> "$verify_summary"
  if [ "$1" != PASS ]; then verify_status=1; fi
}
verify_run() {
  local scope=$1 rc log
  shift
  log="$JET_CI_EVIDENCE_DIR/$scope.log"
  if [ -n "$verify_unsafe" ]; then
    verify_record BLOCKED "$scope" - "$verify_unsafe" "$log"
    return 1
  fi
  if "$@" > "$log" 2>&1; then
    verify_record PASS "$scope" 0 - "$log"
    return 0
  else
    rc=$?
    verify_record FAIL "$scope" "$rc" - "$log"
    # A killed/timed-out command is not an ordinary assertion failure. Do not
    # launch more children without a trustworthy resource/cleanup boundary.
    if [ "$rc" -eq 124 ] || [ "$rc" -ge 128 ]; then
      verify_unsafe="unsafe child exit $rc in $scope"
    fi
    return "$rc"
  fi
}

case "$probe_mode" in
  exit)
    printf '%s\n' "$TMPDIR" "$TMP" "$TEMP" "$JET_VERIFY_TMPDIR"
    exit 0
    ;;
  signal)
    printf '%s\n' "$TMPDIR" "$TMP" "$TEMP" "$JET_VERIFY_TMPDIR"
    kill -TERM "$$"
    exit 143
    ;;
esac

verify_run agent-doc-flags node "$repo/Tools/agent/check-agent-doc-flags.mjs" || true
verify_run core-coverage node "$repo/Tools/agent/core-coverage-census.mjs" --check || true


export JET_CANVAS_PREREQUISITES=strict

canvas_missing=()
for canvas_tool in chromium firefox geckodriver node; do
  canvas_command="$canvas_tool"
  case "$canvas_tool" in
    chromium) canvas_command="${JET_CANVAS_CHROMIUM:-chromium}" ;;
    firefox) canvas_command="${JET_CANVAS_FIREFOX:-firefox}" ;;
    geckodriver) canvas_command="${JET_CANVAS_GECKODRIVER:-geckodriver}" ;;
    node) canvas_command="${JET_CANVAS_NODE:-node}" ;;
  esac
  canvas_resolved="$(command -v -- "$canvas_command" 2>/dev/null || true)"
  canvas_version=""
  if [ -n "$canvas_resolved" ]; then
    canvas_version="$("$canvas_resolved" --version 2>&1 || true)"
  fi
  case "$canvas_tool:$canvas_version" in
    chromium:*Chromium*|chromium:*Chrome*) ;;
    firefox:*Firefox*) ;;
    geckodriver:geckodriver*) ;;
    node:v[0-9]*) ;;
    *) canvas_resolved="" ;;
  esac
  if [ -z "$canvas_resolved" ]; then
    canvas_missing+=("$canvas_tool")
  elif [ "$canvas_tool" = "chromium" ]; then
    export JET_CANVAS_CHROMIUM_RESOLVED="$canvas_resolved"
  elif [ "$canvas_tool" = "firefox" ]; then
    export JET_CANVAS_FIREFOX_RESOLVED="$canvas_resolved"
  elif [ "$canvas_tool" = "geckodriver" ]; then
    export JET_CANVAS_GECKODRIVER_RESOLVED="$canvas_resolved"
  else
    export JET_CANVAS_NODE_RESOLVED="$canvas_resolved"
  fi
done

if ((${#canvas_missing[@]})); then
  missing_list="$(IFS=', '; echo "${canvas_missing[*]}")"
  echo "error: Canvas interaction tests require Chromium, Firefox/geckodriver, and Node; missing: $missing_list. Run Tools/agent/jet-env full Tools/agent/verify-full.sh." >&2
  verify_record BLOCKED canvas-prerequisites - "missing $missing_list" -
fi

export JET_CANVAS_GECKO_SMOKE=1

# tests/cffi_native_matrix.rs::required_native_c_abi_matrix never skips (card
# #436) — it needs a real C compiler/archiver/Rust toolchain to build and run
# a native C ABI fixture. `Tools/agent/jet-env full` provides all three on the host
# target, so full verification runs the matrix for real rather than treating
# it as optional; same strict-missing-means-fail shape as the Canvas block.
cffi_missing=()
for cffi_tool in cc ar rustc; do
  cffi_command="$cffi_tool"
  case "$cffi_tool" in
    cc) cffi_command="${JET_CFFI_CC:-cc}" ;;
    ar) cffi_command="${JET_CFFI_AR:-ar}" ;;
    rustc) cffi_command="${JET_CFFI_RUSTC:-rustc}" ;;
  esac
  cffi_resolved="$(command -v -- "$cffi_command" 2>/dev/null || true)"
  if [ -z "$cffi_resolved" ]; then
    cffi_missing+=("$cffi_tool")
  else
    case "$cffi_tool" in
      cc) export JET_CFFI_CC="$cffi_resolved" ;;
      ar) export JET_CFFI_AR="$cffi_resolved" ;;
      rustc) export JET_CFFI_RUSTC="$cffi_resolved" ;;
    esac
  fi
done

if ((${#cffi_missing[@]})); then
  missing_list="$(IFS=', '; echo "${cffi_missing[*]}")"
  echo "error: the native C ABI matrix requires a C compiler, archiver, and rustc; missing: $missing_list. Run Tools/agent/jet-env full Tools/agent/verify-full.sh." >&2
  verify_record BLOCKED cffi-prerequisites - "missing $missing_list" -
fi

export JET_CFFI_MATRIX_REQUIRED=1
# Native host run: no cross target, alternate linker, or runner wrapper.
export JET_CFFI_ABI="${JET_CFFI_ABI:-default}"

# Focused hostile tests exercise this exact preflight without recursively
# starting the repository's full test suite.
if [ "${JET_VERIFY_CANVAS_PREREQUISITES_ONLY:-}" = "1" ]; then
  exit "$verify_status"
fi

# #805 / D-ONCE-LEDGER1=A: Tower is read-only here; Docs/spec is its rendered
# decision surface. The gate writes only its audit report, never board data.
verify_run tower-hygiene bash "$repo/Tools/ci/tower-hygiene-gate.sh" || true

# The shell manifest comparison is deliberately independent of Nix. CI runs it
# before the Nix-backed stop-line so package selection cannot hide a parity gap.
verify_run shell-parity node "$repo/Tools/agent/verify-jet-shell-parity.js" || true

# Live compiler dogfood is deliberately separate from hermetic fixture tests.
# It downloads real signed closures and builds the compiler in an empty-/nix
# namespace. The coordinator supplies disk-backed scratch and the shared target.
if [ "$(uname -s)" = "Linux" ]; then
  if [ -n "${JETPACK_DOGFOOD_ROOT:-}" ] && [ -n "${JETPACK_DOGFOOD_TARGET_DIR:-}" ]; then
    verify_run live-dogfood timeout --kill-after=30s 1800s cargo test --no-fail-fast --test jetpack_dogfood \
      jet_repository_env_cold_and_offline_without_nix_host_store_or_fixtures \
      -- --exact --ignored --nocapture || true
  else
    verify_record BLOCKED live-dogfood - "fresh bounded dogfood root/target absent" -
  fi
fi

# Cargo's doctest flag, not the presence of a --lib test target, defines the
# default documentation inventory. Disabled libraries must stay disabled.
doc_packages=""
if [ -z "${JET_TEST_SHARD:-}" ]; then
  capture_doc_metadata() {
    cargo metadata --no-deps --format-version 1 > "$JET_CI_EVIDENCE_DIR/doctest-metadata.json"
  }
  select_doc_packages() {
    jq -r '.workspace_members as $members | .packages[] | select(.id as $id | $members | index($id)) | .name as $package | .targets[] | select(.doctest == true) | select(.kind | index("lib") or index("proc-macro") or index("rlib") or index("dylib") or index("cdylib") or index("staticlib")) | $package' \
      "$JET_CI_EVIDENCE_DIR/doctest-metadata.json" > "$JET_CI_EVIDENCE_DIR/doctest-packages.txt"
  }
  if verify_run doctest-metadata capture_doc_metadata && verify_run doctest-inventory select_doc_packages; then
    doc_packages="$(cat "$JET_CI_EVIDENCE_DIR/doctest-packages.txt")"
  else
    verify_unsafe="${verify_unsafe:-doctest input inventory invalid}"
  fi
fi

if ! verify_run nix-eval-stopline "$repo/Tools/agent/verify-nix-eval-stopline.sh"; then
  verify_unsafe="${verify_unsafe:-native Nix authority fence failed}"
fi

# Compile every selected target independently, retaining all diagnostics. A
# failed target blocks only its executable, never later independent targets.
# D-CMD-OVERRIDE1=C: this gate delegates to Cargo; any direct Jet test/bench
# invocation in verification uses --show-default so a file-scoped override cannot
# change the repository gate.
if [ -n "${JET_TEST_SHARD:-}" ]; then
  shard_count="${JET_TEST_SHARD_COUNT:?JET_TEST_SHARD requires JET_TEST_SHARD_COUNT}"
  test_targets="$("$repo/Tools/ci/test-shards.sh" "$JET_TEST_SHARD" "$shard_count")"
else
  test_targets="$("$repo/Tools/ci/test-shards.sh" 0 1)"
fi
if [ -n "${JET_TEST_SHARD:-}" ]; then
  test_targets_repeat="$("$repo/Tools/ci/test-shards.sh" "$JET_TEST_SHARD" "$shard_count")"
else
  test_targets_repeat="$("$repo/Tools/ci/test-shards.sh" 0 1)"
fi
if [ "$test_targets" != "$test_targets_repeat" ]; then
  echo "error: test-target inventory is nondeterministic; refusing a release gate with unstable shard evidence" >&2
  exit 1
fi
if [ -z "$test_targets" ]; then
  echo "error: test-target inventory selected no targets; refusing a false-green verify-full run" >&2
  exit 1
fi
printf '%s\n' "$test_targets" > "$JET_CI_EVIDENCE_DIR/test-targets.txt"
compiled_targets=()
target_index=0
while IFS= read -r test_target; do
  [ -n "$test_target" ] || continue
  target_index=$((target_index + 1))
  verify_target="$test_target"
  read -r -a target_args <<< "$test_target"
  echo "verify-full: compiling test target: cargo test $test_target --no-run --no-fail-fast" >&2
  if verify_run "compile-$target_index" cargo test "${target_args[@]}" --no-run --no-fail-fast "$@"; then
    compiled_targets[$target_index]=1
  fi
done <<< "$test_targets"
verify_target="-"

# #670: once a candidate compiler exists, full verification requires the
# reference-versus-candidate behavioral comparator over the golden, UI and
# differential corpora. Behavior only; stage artifact identity is #815's.
# The canary proves the comparator still detects a planted divergence.
if [ -n "${JET_CANDIDATE_COMPILER:-}" ]; then
  verify_run comparator-canary node "$repo/Tools/agent/compiler-diff.mjs" --canary || true
  verify_run comparator-corpus node "$repo/Tools/agent/compiler-diff.mjs" --candidate "$JET_CANDIDATE_COMPILER" --corpus all --keep-going || true
fi

# Both modes execute the canonical complete inventory (all targets for 0/1,
# the exact selected shard otherwise), even if a sibling did not compile.
target_index=0
while IFS= read -r test_target; do
  [ -n "$test_target" ] || continue
  target_index=$((target_index + 1))
  verify_target="$test_target"
  read -r -a target_args <<< "$test_target"
  # cfg(test) compilation is not a prerequisite for ordinary library docs.
  # Their own Cargo invocation builds/checks the correct independent inputs.
  if [ -z "${JET_TEST_SHARD:-}" ] && [[ "$test_target" == *" --lib" ]]; then
    case $'\n'"$doc_packages"$'\n' in
      *$'\n'"${target_args[1]}"$'\n'*) verify_run "doc-$target_index" cargo test -p "${target_args[1]}" --doc --no-fail-fast "$@" || true ;;
    esac
  fi
  if [ -z "${compiled_targets[$target_index]:-}" ]; then
    verify_record BLOCKED "run-$target_index" - "compile-$target_index did not produce a valid executable" -
    continue
  fi
  # These executable registries explicitly require the unavailable tools.
  # Do not silently let their internal optional-host paths count as coverage.
  if ((${#canvas_missing[@]})) && [[ "$test_target" == *" --test canvas_scenarios" ]]; then
    verify_record BLOCKED "run-$target_index" - canvas-prerequisites -
  elif ((${#cffi_missing[@]})) && [[ "$test_target" == *" --test cffi_native_matrix" ]]; then
    verify_record BLOCKED "run-$target_index" - cffi-prerequisites -
  else
    verify_run "run-$target_index" cargo test "${target_args[@]}" --no-fail-fast "$@" || true
  fi
done <<< "$test_targets"
echo "verify-full: durable complete summary: $verify_summary (aggregate=$verify_status)" >&2
exit "$verify_status"
