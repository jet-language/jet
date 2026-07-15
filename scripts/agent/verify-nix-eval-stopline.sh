#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$repo/tests/fixtures/nix-compat/authority-escape/Cargo.toml"
target="${CARGO_TARGET_DIR:-$repo/target}/nix-eval-authority-escape"
log="${TMPDIR:-$repo/target/test-tmp}/nix-eval-authority-escape.log"
mkdir -p "$(dirname "$log")"

cargo clippy -p jet-nix-eval --all-targets -- \
  -D warnings \
  -D clippy::disallowed_macros \
  -D clippy::disallowed_methods \
  -D clippy::disallowed_types \
  -D clippy::std_instead_of_alloc \
  -D clippy::std_instead_of_core

if CARGO_TARGET_DIR="$target" cargo clippy --manifest-path "$fixture" -- \
  -D warnings \
  -D clippy::disallowed_methods \
  -D clippy::disallowed_types >"$log" 2>&1; then
  echo "error: native Nix evaluator stop-line accepted host process authority" >&2
  exit 1
fi

if ! grep -Fq 'use of a disallowed type `std::process::Command`' "$log"; then
  echo "error: native Nix evaluator escape failed for the wrong reason" >&2
  sed -n '1,160p' "$log" >&2
  exit 1
fi
