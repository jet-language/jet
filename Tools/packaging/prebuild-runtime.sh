#!/usr/bin/env bash
# Prebuild the remaining Rust runtime rlib beside a jet binary, then link the
# Jet-authored runtime object first into the Jet-native runtime pack (#4549).
#
# usage: [JET_PACK_MUSL_LIB=<dir with musl's static libc.a>]
#        [JET_RUNTIME_JET_OBJECT=<already lowered jet_runtime_jet.o>]
#        Tools/packaging/prebuild-runtime.sh <jet-binary> [<prebuilt-dir>]
#
# Builds a seed program with an empty store and JET_RUNTIME_PREBUILD_DIR set, so
# jet writes the runtime rlib it links (default and --release profiles) as
# <prebuilt-dir>/<key>/libjet_runtime.rlib. <prebuilt-dir> defaults to
# jet-runtime/ beside the binary, where `jet build` looks before compiling the
# runtime. The key covers rustc -vV and, for target-cpu=native, this CPU, so the
# entries serve machines with the same rustc and CPU features; any other build
# falls back to compiling the runtime into the store on demand.
#
# The Jet runtime library is assembled/lowered by run-runtime.mjs (or supplied
# through JET_RUNTIME_JET_OBJECT). Its #Export(c) definitions precede the
# remaining --release Rust runtime, Rust standard libraries, musl and libgcc.
# Replaced exports are absent from the native Rust archive (seed-only allocator
# bodies are cfg-disabled); duplicate strong definitions are never stripped.
# The in-process linker (`x64_link_runtime_pack`, run through Tests/run-link.mjs)
# into <prebuilt-dir>/<key>/jet_runtime.pack.o, and <prebuilt-dir>/native.key
# names that key: the Jet-hosted `jet build` reads it (Host.jet
# `jet_cli_runtime_key`) because it has neither rustc nor the runtime's Rust
# text to compute the key itself.
set -euo pipefail
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $0 <jet-binary> [<prebuilt-dir>]" >&2
  exit 64
fi
repo=$(realpath "$(dirname "$0")/../..")
jet=$(realpath "$1")
out=${2:-$(dirname "$jet")/jet-runtime}
work=$(mktemp -d "${TMPDIR:-/tmp}/jet-prebuild-runtime.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$out" "$work/home" "$work/release"
printf 'fn run() {\n    print("hello")\n}\n' > "$work/seed.jet"
for profile in default release; do
  flags=()
  [ "$profile" = release ] && flags=(--release)
  # The release profile's rlib lands in its own directory first so its key is known.
  export_dir=$out
  [ "$profile" = release ] && export_dir=$work/release
  (cd "$work" && HOME="$work/home" JET_STORE_DIR="$work/store" JET_RUNTIME_PREBUILD_DIR="$export_dir" \
    "$jet" build "${flags[@]}" seed.jet > /dev/null)
done
rlibs=("$work"/release/*/libjet_runtime.rlib)
if [ "${#rlibs[@]}" -ne 1 ] || [ ! -f "${rlibs[0]}" ]; then
  echo "prebuild-runtime: the release profile exported no single runtime rlib to $work/release" >&2
  exit 1
fi
key=$(basename "$(dirname "${rlibs[0]}")")
mkdir -p "$out/$key"
[ -f "$out/$key/libjet_runtime.rlib" ] || cp "${rlibs[0]}" "$out/$key/libjet_runtime.rlib"
count=$(find "$out" -name libjet_runtime.rlib | wc -l)
echo "prebuild-runtime: $count runtime rlib(s) in $out; pack key $key"

# The runtime input set: Jet definitions first, followed by remaining layers.
runtime_object=${JET_RUNTIME_JET_OBJECT:-}
if [ -z "$runtime_object" ]; then
  JET="$jet" node "$repo/Compiler/JetBackend/Tests/run-runtime.mjs" "$work/runtime"
  runtime_object=$work/runtime/jet_runtime_jet.o
fi
[ -f "$runtime_object" ] || { echo "prebuild-runtime: no Jet runtime object at $runtime_object" >&2; exit 1; }
[ "$runtime_object" -ef "$out/$key/jet_runtime_jet.o" ] || cp "$runtime_object" "$out/$key/jet_runtime_jet.o"
# Build the full remaining runtime from this tree for the native pack.
node "$repo/Compiler/JetBackend/Tests/emit-allparts-runtime.mjs" "$jet" "$work/allparts.rs"
node "$repo/Compiler/JetBackend/Tests/prepare-call-runtime.mjs" "$work/allparts.rs" "$work/native.rs" --allparts
rustc --edition 2021 --crate-name jet_runtime --crate-type rlib --cfg jet_native_pack -Awarnings \
  -C opt-level=1 -C codegen-units=16 "$work/native.rs" -o "$out/$key/libjet_runtime_native.rlib"
node "$repo/Tools/packaging/check-runtime-exports.mjs" "$out/$key/jet_runtime_jet.o" "$out/$key/libjet_runtime_native.rlib"
sysroot_lib=$(rustc --print sysroot)/lib/rustlib/x86_64-unknown-linux-gnu/lib
archives=("$out/$key/jet_runtime_jet.o" "$out/$key/libjet_runtime_native.rlib")
for name in std panic_unwind object memchr addr2line gimli rustc_demangle std_detect hashbrown rustc_std_workspace_alloc miniz_oxide adler2 unwind cfg_if libc alloc rustc_std_workspace_core core compiler_builtins; do
  rlib=("$sysroot_lib"/lib"$name"-*.rlib)
  if [ ! -f "${rlib[0]}" ]; then
    echo "prebuild-runtime: no lib$name rlib in $sysroot_lib" >&2
    exit 1
  fi
  archives+=("${rlib[0]}")
done
musl=${JET_PACK_MUSL_LIB:?set JET_PACK_MUSL_LIB to the directory holding the static musl libc.a}
[ -f "$musl/libc.a" ] || { echo "prebuild-runtime: no libc.a in $musl" >&2; exit 1; }
archives+=("$musl/libc.a")
for lib in libgcc_eh.a libgcc.a; do
  path=$("${CC:-cc}" -print-file-name="$lib")
  [ -f "$path" ] || { echo "prebuild-runtime: ${CC:-cc} has no $lib" >&2; exit 1; }
  archives+=("$path")
done
node "$repo/Compiler/JetBackend/Tests/run-link.mjs" "$work/link" > /dev/null
(cd "$work/link" && "$jet" run unit.jet -- --pack "$work/jet_runtime.pack.o" "${archives[@]}")
mv "$work/jet_runtime.pack.o" "$out/$key/jet_runtime.pack.o"
printf '%s\n' "$key" > "$work/native.key"
mv "$work/native.key" "$out/native.key"
echo "prebuild-runtime: runtime pack $out/$key/jet_runtime.pack.o; $out/native.key names it"
