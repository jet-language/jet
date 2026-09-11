#!/usr/bin/env bash
set -euo pipefail

# Rebuild the checked-in browser HarfBuzz adapter without ambient tool paths.
# The Nixpkgs revision below is also the repository's flake.lock revision.
SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd -- "$SCRIPT_DIR/../../.." && pwd)
OUTPUT="$ROOT/site/assets/wasm/jet_harfbuzz.wasm"
NIXPKGS_REV=3ed67ec0a4d3c7ab4ae1f04f8ee8df07bfa506a2
NIXPKGS="github:NixOS/nixpkgs/$NIXPKGS_REV"
HARFBUZZ_VERSION=13.2.1
HARFBUZZ_SOURCE_SHA256=6695da3eb7e1be0aa3092fe4d81433a33b47f4519259c759d729e3a9a55c1429
EMSCRIPTEN_VERSION=6.0.8
MESON_VERSION=1.10.2
NINJA_VERSION=1.13.2
EXPECTED_OUTPUT_SHA256=5b8b40963099a0310f1bfdb61c8156c420a6d9a4511ccdf18e487623d5089324

if [[ ${1:-} != --inside ]]; then
    exec nix shell \
        "$NIXPKGS#emscripten" \
        "$NIXPKGS#meson" \
        "$NIXPKGS#ninja" \
        --command "$0" --inside
fi

if [[ "$(emcc --version)" != *"${EMSCRIPTEN_VERSION}"* ]]; then
    printf 'unexpected emcc version (wanted %s)\n' "$EMSCRIPTEN_VERSION" >&2
    exit 1
fi
if [[ "$(meson --version)" != "$MESON_VERSION" ]]; then
    printf 'unexpected meson version (wanted %s)\n' "$MESON_VERSION" >&2
    exit 1
fi
if [[ "$(ninja --version)" != "$NINJA_VERSION" ]]; then
    printf 'unexpected ninja version (wanted %s)\n' "$NINJA_VERSION" >&2
    exit 1
fi

SCRATCH_ROOT=${XDG_CACHE_HOME:-$ROOT/.cache}/jet-harfbuzz
mkdir -p "$SCRATCH_ROOT"
SCRATCH=$(mktemp -d "$SCRATCH_ROOT/build.XXXXXX")
trap 'rm -rf "$SCRATCH"' EXIT
export EM_CACHE="$SCRATCH/em-cache"
export XDG_CACHE_HOME="$SCRATCH/xdg-cache"
mkdir -p "$EM_CACHE" "$XDG_CACHE_HOME"

SOURCE_ARCHIVE=$(nix build --no-link --print-out-paths "$NIXPKGS#harfbuzz.src")
read -r SOURCE_SHA _ < <(sha256sum "$SOURCE_ARCHIVE")
if [[ "$SOURCE_SHA" != "$HARFBUZZ_SOURCE_SHA256" ]]; then
    printf 'unexpected HarfBuzz source hash: %s\n' "$SOURCE_SHA" >&2
    exit 1
fi
mkdir -p "$SCRATCH/source"
tar -xf "$SOURCE_ARCHIVE" -C "$SCRATCH/source"
SOURCE="$SCRATCH/source/harfbuzz-$HARFBUZZ_VERSION"
BUILD="$SCRATCH/build"
CROSS="$SCRATCH/emscripten.ini"
cat > "$CROSS" <<'EOF'
[binaries]
c = 'emcc'
cpp = 'em++'
ar = 'emar'
ranlib = 'emranlib'

[host_machine]
system = 'emscripten'
cpu_family = 'wasm32'
cpu = 'wasm32'
endian = 'little'
EOF

meson setup "$BUILD" "$SOURCE" \
    --cross-file "$CROSS" \
    --buildtype=release \
    --wrap-mode=nodownload \
    -Ddefault_library=static \
    -Dglib=disabled \
    -Dgobject=disabled \
    -Dcairo=disabled \
    -Dchafa=disabled \
    -Dpng=disabled \
    -Dzlib=disabled \
    -Dicu=disabled \
    -Dgraphite2=disabled \
    -Dfreetype=disabled \
    -Dfontations=disabled \
    -Dgdi=disabled \
    -Ddirectwrite=disabled \
    -Dcoretext=disabled \
    -Dharfrust=disabled \
    -Dkbts=disabled \
    -Dwasm=disabled \
    -Draster=disabled \
    -Dvector=disabled \
    -Dsubset=disabled \
    -Dtests=disabled \
    -Dintrospection=disabled \
    -Ddocs=disabled \
    -Dutilities=disabled \
    -Dbenchmark=disabled \
    -Ddoc_tests=false
ninja -C "$BUILD" src/libharfbuzz.a

EXPORTS='["_jet_hb_abi_version","_jet_hb_alloc","_jet_hb_free","_jet_hb_blob_create","_jet_hb_blob_destroy","_jet_hb_face_create","_jet_hb_face_get_glyph_count","_jet_hb_face_destroy","_jet_hb_font_create","_jet_hb_font_destroy","_jet_hb_font_set_scale","_jet_hb_ot_font_set_funcs","_jet_hb_buffer_create","_jet_hb_buffer_destroy","_jet_hb_buffer_add_utf8","_jet_hb_buffer_guess_segment_properties","_jet_hb_shape","_jet_hb_buffer_get_length","_jet_hb_buffer_get_glyph_infos","_jet_hb_buffer_get_glyph_positions"]'
emcc "$SCRIPT_DIR/jet_harfbuzz_bridge.c" "$BUILD/src/libharfbuzz.a" \
    -I"$SOURCE/src" \
    -I"$BUILD/src" \
    -I"$BUILD" \
    -O2 \
    -std=c11 \
    --no-entry \
    -sSTANDALONE_WASM=1 \
    -sFILESYSTEM=0 \
    -sALLOW_MEMORY_GROWTH=1 \
    -sINITIAL_MEMORY=33554432 \
    -sMAXIMUM_MEMORY=268435456 \
    -sSTACK_SIZE=1048576 \
    -sEXPORTED_FUNCTIONS="$EXPORTS" \
    -sEXPORTED_RUNTIME_METHODS='[]' \
    -o "$SCRATCH/jet_harfbuzz.wasm"

read -r OUTPUT_SHA _ < <(sha256sum "$SCRATCH/jet_harfbuzz.wasm")
if [[ "$OUTPUT_SHA" != "$EXPECTED_OUTPUT_SHA256" ]]; then
    printf 'unexpected adapter hash: %s\n' "$OUTPUT_SHA" >&2
    exit 1
fi
mkdir -p "$(dirname -- "$OUTPUT")"
install -m 0644 "$SCRATCH/jet_harfbuzz.wasm" "$OUTPUT"
printf 'wrote %s (%s)\n' "$OUTPUT" "$OUTPUT_SHA"
