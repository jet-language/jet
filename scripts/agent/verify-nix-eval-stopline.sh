#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$repo/tests/fixtures/nix-compat/authority-escape/Cargo.toml"
build_fixture="$repo/tests/fixtures/nix-compat/build-script-escape/Cargo.toml"
manifest="$repo/crates/jet-nix-eval/Cargo.toml"
target="${CARGO_TARGET_DIR:-$repo/target}/nix-eval-authority-escape"
log="${TMPDIR:-$repo/target/test-tmp}/nix-eval-authority-escape.log"
mkdir -p "$(dirname "$log")"

if ! grep -Eq '^build[[:space:]]*=[[:space:]]*false[[:space:]]*$' "$manifest"; then
  echo "error: native Nix evaluator must set package.build = false" >&2
  exit 1
fi
if [ -e "$repo/crates/jet-nix-eval/build.rs" ]; then
  echo "error: native Nix evaluator cannot contain a build script" >&2
  exit 1
fi
if cargo metadata --manifest-path "$manifest" --no-deps --format-version 1 \
  | jq -e '.packages[] | select(.name == "jet-nix-eval") | .targets[] | select(.kind | index("custom-build"))' \
    >/dev/null; then
  echo "error: Cargo resolved a native Nix evaluator build-script target" >&2
  exit 1
fi

cargo clippy -p jet-nix-eval --all-targets -- \
  -D warnings \
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

for authority in \
  'std::net::TcpStream' \
  'std::net::ToSocketAddrs' \
  'std::os::unix::net::UnixStream'; do
  if ! grep -Fq "use of a disallowed type \`$authority\`" "$log"; then
    echo "error: native Nix evaluator escape did not reject $authority" >&2
    sed -n '1,200p' "$log" >&2
    exit 1
  fi
done

CARGO_TARGET_DIR="$target-build" cargo check --manifest-path "$build_fixture"
