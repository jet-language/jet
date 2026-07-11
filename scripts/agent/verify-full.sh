#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
"$repo/scripts/agent/clean-nix-tmp.sh"
tmp="${JET_VERIFY_TMPDIR:-$repo/target/test-tmp}"
mkdir -p "$tmp"

export TMPDIR="$tmp"
export JET_TEST_JOBS="${JET_TEST_JOBS:-16}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-$JET_TEST_JOBS}"
exec cargo test "$@"
