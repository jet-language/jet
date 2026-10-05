#!/usr/bin/env bash
# Prebuild the Jet runtime rlib for the default build-fact set beside a jet binary.
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
