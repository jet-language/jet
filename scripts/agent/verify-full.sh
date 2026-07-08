#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tmp="${TMPDIR:-$repo/target/test-tmp}"
mkdir -p "$tmp"

export TMPDIR="$tmp"
exec cargo test "$@"
