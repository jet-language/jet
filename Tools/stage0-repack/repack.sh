#!/usr/bin/env bash
# Repackage a kept stage-zero run without its frontend, then optionally check
# the repacked backend project.
#
# usage: Tools/stage0-repack/repack.sh [--check] [--emission-only --crates <dir>] \
#          --source-root <dir> --emission-pin <rev> --generator-pin <rev> [--resume-pin <rev>] \
#          <kept-dir> <copy-dir> [<compiler-project>]
#
# <kept-dir> is a `JET_STAGE_ZERO_KEEP` directory (reference.rs, stage-zero.rs,
# stage-zero.image, stage-zero.ffi, stage-zero.stamp, stage-zero/). It is only
# read: the script copies it to <copy-dir> (which must not exist yet) and
# repacks the copy. The harness links the workspace crates of the tree the kept
# run was built from (the `jet` path in <kept-dir>/stage-zero/Cargo.toml, or
# --crates): the compiler image codec and the emitter must be that run's
# (<emission-pin>). The Host/Runner generators come from --source-root, which
# must be an export of <generator-pin> (every Compiler/Bootstrap file is
# checked against the pin), so a generator edit rebuilds only the harness
# crate.
#
# --emission-only copies everything but the backend project stage-zero/ and
# repacks stage-zero.rs only: for a run stopped after emission, whose
# `JET_STAGE_ZERO_RESUME` writes the backend project from stage-zero.rs. It
# needs --crates (the emission pin's source tree).
#
# Provenance guard (provenance.mjs): `git diff --name-only <emission-pin>
# <generator-pin>` must touch only Compiler/Bootstrap/Host/**,
# Compiler/Bootstrap/Tests.rs and Tools/**; otherwise the repack is refused.
# After packaging, the copy may differ from <kept-dir> only in stage-zero.rs,
# stage-zero/generated.rs and stage-zero/src/main.rs (plus the harness's own
# files), and the unit/shard crate inputs (reference.rs prefix and the
# unit/runtime sections) must be unchanged; the result is recorded in
# <copy-dir>/repack-provenance.json. --resume-pin records the tree the later
# `JET_STAGE_ZERO_RESUME` builds the backend crates from (its diff may also
# touch the libjet_codegen Observe prelude: crates/jet-codegen/{build.rs,
# src/lib.rs,src/Prelude/Observe.rs} and crates/jet-comptime/build.rs;
# anything else is refused).
#
# --check then runs `cargo check` on <copy-dir>/stage-zero with warnings capped
# (log: <copy-dir>/repack-check.log). REPACK_TARGET_DIR overrides the cargo
# target dir (default: this checkout's target/).
# Heavy: run it under a memory cap, e.g. a `systemd-run --user -p MemoryMax=24G`
# unit (packaging decodes the whole compiler MIR; 8G is not enough).
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
usage="usage: repack.sh [--check] [--emission-only --crates <dir>] --source-root <dir> --emission-pin <rev> --generator-pin <rev> [--resume-pin <rev>] <kept-dir> <copy-dir> [<compiler-project>]"
resume_pin=
check=0
emission_only=0
crates=
source_root=
emission_pin=
generator_pin=
while [ $# -gt 0 ]; do
  case "$1" in
    --check) check=1; shift ;;
    --emission-only) emission_only=1; shift ;;
    --crates) crates="$(realpath -e -- "${2:?$usage}")"; shift 2 ;;
    --source-root) source_root="${2:?$usage}"; shift 2 ;;
    --emission-pin) emission_pin="${2:?$usage}"; shift 2 ;;
    --generator-pin) generator_pin="${2:?$usage}"; shift 2 ;;
    --resume-pin) resume_pin="${2:?$usage}"; shift 2 ;;
    --) shift; break ;;
    -*) echo "$usage" >&2; exit 2 ;;
    *) break ;;
  esac
done
[ -n "$source_root" ] && [ -n "$emission_pin" ] && [ -n "$generator_pin" ] || { echo "$usage" >&2; exit 2; }
[ $# -ge 2 ] || { echo "$usage" >&2; exit 2; }
kept="$(realpath -e -- "$1")"
copy="$(realpath -m -- "$2")"
shift 2
sources="$(realpath -e -- "$source_root")"
jobs="${CARGO_BUILD_JOBS:-4}"
required="reference.rs stage-zero.rs stage-zero.image stage-zero.ffi stage-zero.stamp"
[ "$emission_only" = 1 ] || required="$required stage-zero/Cargo.toml stage-zero/src/main.rs"
for name in $required; do
  [ -s "$kept/$name" ] || { echo "repack: $kept is not a kept stage-zero dir (no $name)" >&2; exit 2; }
done
[ ! -e "$copy" ] || { echo "repack: copy dir $copy already exists; give a fresh path" >&2; exit 2; }
case "$copy/" in "$kept/"*) echo "repack: copy dir $copy is inside the kept dir" >&2; exit 2 ;; esac
if [ "$emission_only" = 1 ]; then
  [ -n "$crates" ] || { echo "repack: --emission-only needs --crates <emission-pin source tree>" >&2; exit 2; }
  [ "$check" = 0 ] || { echo "repack: --check needs the backend project; drop --emission-only" >&2; exit 2; }
else
  kept_crates="$(sed -n 's/^jet = { path = "\(.*\)" }$/\1/p' "$kept/stage-zero/Cargo.toml")"
  [ -n "$kept_crates" ] || { echo "repack: no \`jet\` path dependency in $kept/stage-zero/Cargo.toml" >&2; exit 2; }
  [ -z "$crates" ] || [ "$crates" = "$(realpath -e -- "$kept_crates")" ] ||
    { echo "repack: --crates $crates is not the kept project's crate tree $kept_crates" >&2; exit 2; }
  crates="$kept_crates"
fi

# Refuse before touching anything: pins, allowed diff, and the source root.
guard="$(mktemp)"
trap 'rm -f "$guard"' EXIT
node "$repo/Tools/stage0-repack/provenance.mjs" guard "$repo" "$emission_pin" "$generator_pin" "$sources" "$crates" "$guard" ${resume_pin:+"$resume_pin"}

start=$SECONDS
mkdir -p "$copy"
for entry in "$kept"/* "$kept"/.[!.]*; do
  [ -e "$entry" ] || continue
  [ "$emission_only" = 1 ] && [ "$(basename -- "$entry")" = stage-zero ] && continue
  cp -a --reflink=auto -- "$entry" "$copy/"
done
echo "repack: copied $kept -> $copy in $((SECONDS - start))s" >&2

# The harness manifest lives beside the repacked project: it names the kept
# run's crates and this checkout's harness sources.
harness="$copy/repack"
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
jet-evalhost = { path = "$crates/crates/jet-evalhost" }
jet-foundation = { path = "$crates/crates/jet-foundation" }
jet-pkg-model = { path = "$crates/crates/jet-pkg-model" }
jet-store = { path = "$crates/crates/jet-store" }

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
# Every repack builds the same target/debug binary from its own generator
# root: build and take a private copy under one lock, so concurrent repacks
# never run each other's generators. REPACK_TARGET_DIR keeps one kept-crates
# tree's built dependencies apart from another's.
target="${REPACK_TARGET_DIR:-$repo/target}"
mkdir -p "$target"
exec 9>"$target/.stage0-repack.lock"
flock 9
# Some kept-tree crates deny warnings that their lib-only feature set trips.
"$repo/Tools/agent/jet-env" env JET_REPACK_SOURCE_ROOT="$sources" RUSTFLAGS=--cap-lints=warn CARGO_TARGET_DIR="$target" \
  CARGO_BUILD_JOBS="$jobs" cargo build --quiet --manifest-path "$harness/Cargo.toml" 2>"$harness/build.log" || {
  grep -E '^error' -A6 "$harness/build.log" >&2 || true
  echo "repack: harness build failed; log $harness/build.log" >&2
  exit 1
}
cp "$target/debug/jet-stage0-repack" "$harness/jet-stage0-repack"
flock -u 9
echo "repack: harness built in $((SECONDS - start))s (generators: $sources; crates: $crates)" >&2
start=$SECONDS
"$harness/jet-stage0-repack" "$copy" "$@"
echo "repack: packaged in $((SECONDS - start))s" >&2
start=$SECONDS
node "$repo/Tools/stage0-repack/provenance.mjs" finish "$guard" "$kept" "$copy" "$emission_only"
echo "repack: provenance verified in $((SECONDS - start))s; $copy/repack-provenance.json" >&2
if [ "$check" = 1 ]; then
  start=$SECONDS
  status=0
  "$repo/Tools/agent/jet-env" env RUSTFLAGS=--cap-lints=warn CARGO_TARGET_DIR="$target" CARGO_BUILD_JOBS="$jobs" \
    cargo check --manifest-path "$copy/stage-zero/Cargo.toml" --message-format short >"$copy/repack-check.log" 2>&1 || status=$?
  errors=$(grep -c ': error' "$copy/repack-check.log" || true)
  echo "repack: cargo check exit $status, $errors error lines, $((SECONDS - start))s; log $copy/repack-check.log" >&2
  exit "$status"
fi
