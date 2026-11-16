#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
jet=${JET:-"$root/../../../../Tools/agent/JetEnv.jet"}
cc=${CC:-cc}
cxx=${CXX:-c++}
rustc=${RUSTC:-rustc}
zig=${ZIG:-zig}
go=${GO:-go}
python=${PYTHON:-python3}
node=${NODE:-node}

for language in C Cpp Rust Zig Go Python JavaScript; do
    (cd "$root/$language" && "$jet" build --lib guest.jet)
done

"$cc" -std=c11 -I"$root/C/.jet/build" \
    "$root/C/Host.c" "$root/C/.jet/build/libprojection_c.a" \
    -ldl -lpthread -lm -o "$root/C/host"
"$root/C/host"

"$cxx" -std=c++17 -I"$root/C/.jet/build" -I"$root/C/.jet/build/bindings" \
    "$root/Cpp/Host.cpp" "$root/Cpp/.jet/build/libprojection_cpp.a" \
    -pthread -ldl -lm -o "$root/Cpp/host"
"$root/Cpp/host"

"$rustc" --edition=2021 "$root/Rust/Host.rs" \
    -L "$root/Rust/.jet/build" -l static=projection_rust \
    -C link-arg=-ldl -C link-arg=-lpthread -C link-arg=-lm \
    -o "$root/Rust/host"
"$root/Rust/host"

"$zig" build-exe "$root/Zig/Host.zig" \
    -I "$root/Zig/.jet/build" -L "$root/Zig/.jet/build" -lprojection_zig \
    -lc -ldl -lpthread -lm -femit-bin="$root/Zig/host"
"$root/Zig/host"

(
    cd "$root/Go"
    CGO_ENABLED=1 GO111MODULE=on \
        CGO_CFLAGS="-I$root/Go/.jet/build" \
        CGO_LDFLAGS="-L$root/Go/.jet/build -lprojection_go -ldl -lpthread -lm" \
        "$go" run .
)

(
    cd "$root/Python"
    PYTHONPATH="$root/Python/.jet/build/bindings" \
        LD_LIBRARY_PATH="$root/Python/.jet/build${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        "$python" Host.py
)

node_addon=${JET_NODE_ADDON:-"$root/JavaScript/.jet/build/libprojection_javascript.node"}
if [ -z "${JET_NODE_ADDON:-}" ]; then
    "$cc" -std=c11 -shared -fPIC -I"$root/JavaScript/.jet/build" \
        "$root/JavaScript/Addon.c" "$root/JavaScript/.jet/build/libprojection_javascript.so" \
        -ldl -lpthread -lm -Wl,-rpath,'$ORIGIN' \
        -o "$node_addon"
fi
(
    cd "$root/JavaScript"
    LD_LIBRARY_PATH="$root/JavaScript/.jet/build${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        JET_NODE_ADDON="$node_addon" \
        "$node" Host.mjs
)
