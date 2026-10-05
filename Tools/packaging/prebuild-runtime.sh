#!/usr/bin/env bash
# Prebuild the Jet runtime rlib for the default build-fact set beside a jet binary,
# and install the stripped Cranelift dev-build runner (#3953) next to it.
#
# usage: Tools/packaging/prebuild-runtime.sh <jet-binary> [<prebuilt-dir>]
#
# Builds a seed program with an empty store and JET_RUNTIME_PREBUILD_DIR set, so
# jet writes the runtime rlib it links (default and --release profiles) as
# <prebuilt-dir>/<key>/libjet_runtime.rlib. <prebuilt-dir> defaults to
# jet-runtime/ beside the binary, where `jet build` looks before compiling the
# runtime. The key covers rustc -vV and, for target-cpu=native, this CPU, so the
# entries serve machines with the same rustc and CPU features; any other build
# falls back to compiling the runtime into the store on demand.
#
# Every dev `jet build` copies the runner `jet-aot-rt` (built beside jet by
# `cargo build --release --bin jet --bin jet-aot-rt`) into its output, and the
# build cache stores and verifies that copy, so the runner ships without its
# symbol table: <prebuilt-dir>/jet-aot-rt, which `jet build` prefers.
set -euo pipefail
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 <jet-binary> [<prebuilt-dir>]" >&2
  exit 64
fi
jet=$(realpath "$1")
out=${2:-$(dirname "$jet")/jet-runtime}
work=$(mktemp -d "${TMPDIR:-/tmp}/jet-prebuild-runtime.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$out" "$work/home"
printf 'fn run() {\n    print("hello")\n}\n' > "$work/seed.jet"
for profile in default release; do
  flags=()
  [ "$profile" = release ] && flags=(--release)
  (cd "$work" && HOME="$work/home" JET_STORE_DIR="$work/store" JET_RUNTIME_PREBUILD_DIR="$out" \
    "$jet" build "${flags[@]}" seed.jet > /dev/null)
done
count=$(find "$out" -name libjet_runtime.rlib | wc -l)
if [ "$count" -eq 0 ]; then
  echo "prebuild-runtime: jet exported no runtime rlib to $out" >&2
  exit 1
fi
echo "prebuild-runtime: $count runtime rlib(s) in $out"
runner=$(dirname "$jet")/jet-aot-rt
if [ ! -x "$runner" ]; then
  echo "prebuild-runtime: no dev-build runner at $runner; build it with the jet binary" >&2
  exit 1
fi
strip_tool=$(command -v llvm-strip || command -v strip) || {
  echo "prebuild-runtime: neither llvm-strip nor strip is on PATH" >&2
  exit 1
}
"$strip_tool" -o "$out/jet-aot-rt" "$runner"
echo "prebuild-runtime: stripped runner $out/jet-aot-rt ($(wc -c < "$out/jet-aot-rt") bytes)"
