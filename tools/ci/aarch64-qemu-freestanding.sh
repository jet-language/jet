#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

target="aarch64-unknown-linux-gnu"
src="examples/features/61_freestanding.jet"
rust_src="build/61_freestanding.rs"
bin="build/61_freestanding-aarch64"
expected="examples/features/expected/61_freestanding.out"

# Unset any cross-compiler env vars so the host build uses the host toolchain.
unset CC CXX

cargo build

# Generate sema-checked freestanding Rust through Jet. The host build also
# proves the freestanding front-end gates before we cross-compile the emitted
# Rust with the target-aware rustup toolchain.
jet build --emit-rust --freestanding "$src" >/tmp/jet-freestanding-rust.txt
test -f "$rust_src"

rustc_aarch64="${RUSTC_AARCH64:-$HOME/.cargo/bin/rustc}"
if [ ! -x "$rustc_aarch64" ]; then
  rustc_aarch64="$(command -v rustc)"
fi

# Debian/Ubuntu name the cross gcc aarch64-linux-gnu-gcc; Nix pkgsCross uses
# the full GNU triplet. Accept either.
linker="$(command -v aarch64-unknown-linux-gnu-gcc || command -v aarch64-linux-gnu-gcc || true)"
if [ -z "$linker" ]; then
  echo "no aarch64 cross gcc found (tried aarch64-unknown-linux-gnu-gcc, aarch64-linux-gnu-gcc)" >&2
  exit 1
fi

"$rustc_aarch64" \
  --edition 2021 \
  --target "$target" \
  -C linker="$linker" \
  -C opt-level=z \
  -C panic=abort \
  -C strip=symbols \
  "$rust_src" \
  -o "$bin"

actual="$(qemu-aarch64 "$bin")"
want="$(cat "$expected")"
if [ "$actual" != "$want" ]; then
  printf 'aarch64/QEMU output mismatch\nexpected: %s\nactual:   %s\n' "$want" "$actual" >&2
  exit 1
fi

printf '%s\n' "$actual"
