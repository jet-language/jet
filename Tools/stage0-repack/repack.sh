#!/usr/bin/env bash
# Repackage a kept stage-zero run without its frontend, then optionally check
# the kept backend project.
#
# usage: Tools/stage0-repack/repack.sh [--check] <keep-dir> [<compiler-project>]
#
# <keep-dir> is a `JET_STAGE_ZERO_KEEP` directory (reference.rs, stage-zero.rs,
# stage-zero.image, stage-zero.ffi, stage-zero/). The harness links the
# workspace crates of the tree the kept run was built from (the `jet` path in
# <keep-dir>/stage-zero/Cargo.toml): the compiler image codec and the emitter
# must be that run's. The Host/Runner generators come from
# JET_REPACK_SOURCE_ROOT (default: this checkout), so a generator edit
# rebuilds only the harness crate. --check then runs `cargo check` on
# <keep-dir>/stage-zero with warnings capped (log: <keep-dir>/repack-check.log).
# Heavy: run it through the memory lane, e.g.
#   ~/.cache/jet-dev/laneB.sh 12 Tools/stage0-repack/repack.sh --check ~/.cache/jet-dev/stage0
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
check=0
if [ "${1:-}" = "--check" ]; then
  check=1
  shift
fi
keep="$(realpath -- "${1:?usage: repack.sh [--check] <keep-dir> [<compiler-project>]}")"
shift
jobs="${CARGO_BUILD_JOBS:-4}"
sources="$(realpath -- "${JET_REPACK_SOURCE_ROOT:-$repo}")"
crates="$(sed -n 's/^jet = { path = "\(.*\)" }$/\1/p' "$keep/stage-zero/Cargo.toml")"
[ -n "$crates" ] || { echo "repack: no \`jet\` path dependency in $keep/stage-zero/Cargo.toml" >&2; exit 2; }

# The harness manifest lives beside the kept project: it names the kept run's
# crates and this checkout's harness sources.
harness="$keep/repack"
mkdir -p "$harness"
cat >"$harness/Cargo.toml" <<EOF
[package]
name = "jet-stage0-repack"
version = "0.0.0"
edition = "2021"
publish = false
build = "$repo/Tools/stage0-repack/build.rs"

[[bin]]
name = "jet-stage0-repack"
path = "$repo/Tools/stage0-repack/src/main.rs"

[dependencies]
jet-codegen = { path = "$crates/crates/jet-codegen" }
jet-driver = { path = "$crates/crates/jet-driver" }
jet-foundation = { path = "$crates/crates/jet-foundation" }
jet-jit = { path = "$crates/crates/jet-jit" }
jet-pkg-model = { path = "$crates/crates/jet-pkg-model" }
jet-store = { path = "$crates/crates/jet-store" }

[patch.crates-io]
cranelift-jit = { path = "$crates/crates/vendor/cranelift-jit-0.112.3" }

[profile.dev]
debug = false

# Decoding the image and describing its AOT metadata walk the whole compiler
# MIR: optimize the libraries once; the harness crate (the Host modules under
# edit) stays unoptimized so each generator edit recompiles quickly.
[profile.dev.package."*"]
opt-level = 2
debug = false

[profile.dev.build-override]
opt-level = 3

[workspace]
EOF
# Same dependency versions as the kept run's build (as the backend project does).
cp "$crates/Cargo.lock" "$harness/Cargo.lock"

start=$SECONDS
# Every keep dir builds the same target/debug binary from its own generator
# root: build and take a private copy under one lock, so concurrent repacks
# never run each other's generators.
exec 9>"$repo/target/.stage0-repack.lock"
flock 9
# Some kept-tree crates deny warnings that their lib-only feature set trips.
"$repo/Tools/agent/jet-env" env JET_REPACK_SOURCE_ROOT="$sources" RUSTFLAGS=--cap-lints=warn CARGO_TARGET_DIR="$repo/target" \
  CARGO_BUILD_JOBS="$jobs" cargo build --quiet --manifest-path "$harness/Cargo.toml" 2>"$harness/build.log" || {
  grep -E '^error' -A6 "$harness/build.log" >&2 || true
  echo "repack: harness build failed; log $harness/build.log" >&2
  exit 1
}
cp "$repo/target/debug/jet-stage0-repack" "$harness/jet-stage0-repack"
flock -u 9
echo "repack: harness built in $((SECONDS - start))s (generators: $sources; crates: $crates)" >&2
start=$SECONDS
"$harness/jet-stage0-repack" "$keep" "$@"
echo "repack: packaged in $((SECONDS - start))s" >&2
if [ "$check" = 1 ]; then
  start=$SECONDS
  status=0
  "$repo/Tools/agent/jet-env" env RUSTFLAGS=--cap-lints=warn CARGO_TARGET_DIR="$repo/target" CARGO_BUILD_JOBS="$jobs" \
    cargo check --manifest-path "$keep/stage-zero/Cargo.toml" --message-format short >"$keep/repack-check.log" 2>&1 || status=$?
  errors=$(grep -c ': error' "$keep/repack-check.log" || true)
  echo "repack: cargo check exit $status, $errors error lines, $((SECONDS - start))s; log $keep/repack-check.log" >&2
  exit "$status"
fi
