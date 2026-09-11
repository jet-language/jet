#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
jet=${JET:-"$root/../../../scripts/agent/jet-env"}
cc=${CC:-cc}
cxx=${CXX:-c++}
rustc=${RUSTC:-rustc}
zig=${ZIG:-zig}
go=${GO:-go}
python=${PYTHON:-python3}
node=${NODE:-node}

for language in c cpp rust zig go python javascript; do
    (cd "$root/$language" && "$jet" build --lib guest.jet)
done

"$cc" -std=c11 -I"$root/c/target" \
    "$root/c/host.c" "$root/c/target/libprojection_c.a" \
    -ldl -lpthread -lm -o "$root/c/host"
"$root/c/host"

"$cxx" -std=c++17 -I"$root/cpp/target" -I"$root/cpp/target/bindings" \
    "$root/cpp/host.cpp" "$root/cpp/target/libprojection_cpp.a" \
    -pthread -ldl -lm -o "$root/cpp/host"
"$root/cpp/host"

"$rustc" --edition=2021 "$root/rust/host.rs" \
    -L "$root/rust/target" -l static=projection_rust \
    -C link-arg=-ldl -C link-arg=-lpthread -C link-arg=-lm \
    -o "$root/rust/host"
"$root/rust/host"

"$zig" build-exe "$root/zig/host.zig" \
    -I "$root/zig/target" -L "$root/zig/target" -lprojection_zig \
    -lc -ldl -lpthread -lm -femit-bin="$root/zig/host"
"$root/zig/host"

(
    cd "$root/go"
    CGO_ENABLED=1 GO111MODULE=on \
        CGO_CFLAGS="-I$root/go/target" \
        CGO_LDFLAGS="-L$root/go/target -lprojection_go -ldl -lpthread -lm" \
        "$go" run .
)

(
    cd "$root/python"
    PYTHONPATH="$root/python/target/bindings" \
        LD_LIBRARY_PATH="$root/python/target${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        "$python" host.py
)

node_addon=${JET_NODE_ADDON:-"$root/javascript/target/libprojection_javascript.node"}
if [ -z "${JET_NODE_ADDON:-}" ]; then
    "$cc" -std=c11 -shared -fPIC -I"$root/javascript/target" \
        "$root/javascript/addon.c" "$root/javascript/target/libprojection_javascript.so" \
        -ldl -lpthread -lm -Wl,-rpath,'$ORIGIN' \
        -o "$node_addon"
fi
(
    cd "$root/javascript"
    LD_LIBRARY_PATH="$root/javascript/target${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
        JET_NODE_ADDON="$node_addon" \
        "$node" host.mjs
)
