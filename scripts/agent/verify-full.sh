#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [ "${JET_NIX_TMP_CLEANED:-}" != "1" ]; then
  "$repo/scripts/agent/clean-nix-tmp.sh"
fi
export JET_NIX_TMP_CLEANED=1
tmp="${JET_VERIFY_TMPDIR:-$repo/target/test-tmp}"
mkdir -p "$tmp"

export TMPDIR="$tmp"
export JET_TEST_JOBS="${JET_TEST_JOBS:-16}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-$JET_TEST_JOBS}"

export JET_CANVAS_PREREQUISITES=strict

canvas_missing=()
for canvas_tool in chromium node; do
  canvas_command="$canvas_tool"
  case "$canvas_tool" in
    chromium) canvas_command="${JET_CANVAS_CHROMIUM:-chromium}" ;;
    node) canvas_command="${JET_CANVAS_NODE:-node}" ;;
  esac
  canvas_resolved="$(command -v -- "$canvas_command" 2>/dev/null || true)"
  canvas_version=""
  if [ -n "$canvas_resolved" ]; then
    canvas_version="$("$canvas_resolved" --version 2>&1 || true)"
  fi
  case "$canvas_tool:$canvas_version" in
    chromium:*Chromium*|chromium:*Chrome*) ;;
    node:v[0-9]*) ;;
    *) canvas_resolved="" ;;
  esac
  if [ -z "$canvas_resolved" ]; then
    canvas_missing+=("$canvas_tool")
  elif [ "$canvas_tool" = "chromium" ]; then
    export JET_CANVAS_CHROMIUM_RESOLVED="$canvas_resolved"
  else
    export JET_CANVAS_NODE_RESOLVED="$canvas_resolved"
  fi
done

if ((${#canvas_missing[@]})); then
  missing_list="$(IFS=', '; echo "${canvas_missing[*]}")"
  echo "error: Canvas interaction tests require Chromium and Node; missing: $missing_list. Run scripts/agent/jet-env full scripts/agent/verify-full.sh." >&2
  exit 1
fi

# tests/cffi_native_matrix.rs::required_native_c_abi_matrix never skips (card
# #436) — it needs a real C compiler/archiver/Rust toolchain to build and run
# a native C ABI fixture. `scripts/agent/jet-env full` provides all three on the host
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
  echo "error: the native C ABI matrix requires a C compiler, archiver, and rustc; missing: $missing_list. Run scripts/agent/jet-env full scripts/agent/verify-full.sh." >&2
  exit 1
fi

export JET_CFFI_MATRIX_REQUIRED=1
# Native host run: no cross target, alternate linker, or runner wrapper.
export JET_CFFI_ABI="${JET_CFFI_ABI:-default}"

# Focused hostile tests exercise this exact preflight without recursively
# starting the repository's full test suite.
if [ "${JET_VERIFY_CANVAS_PREREQUISITES_ONLY:-}" = "1" ]; then
  exit 0
fi

"$repo/scripts/agent/verify-nix-eval-stopline.sh"

exec cargo test "$@"
