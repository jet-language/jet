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
  if ! command -v -- "$canvas_command" >/dev/null 2>&1; then
    canvas_missing+=("$canvas_tool")
  fi
done

if ((${#canvas_missing[@]})); then
  missing_list="$(IFS=', '; echo "${canvas_missing[*]}")"
  echo "error: Canvas interaction tests require Chromium and Node; missing: $missing_list. Run full verification inside 'nix develop'." >&2
  exit 1
fi

# Focused hostile tests exercise this exact preflight without recursively
# starting the repository's full test suite.
if [ "${JET_VERIFY_CANVAS_PREREQUISITES_ONLY:-}" = "1" ]; then
  exit 0
fi

exec cargo test "$@"
