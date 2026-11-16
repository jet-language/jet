#!/usr/bin/env bash
# Rebuilds the hot-text entry's stand-in compiler and runtime pack from
# main.c and pack.c: compiler/jet and compiler/jet-runtime/jet_runtime.pack.o.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../compiler"
mkdir -p "$out/jet-runtime"
cc -O2 -fno-pie -c -o "$out/jet-runtime/jet_runtime.pack.o" "$here/pack.c"
cc -O2 -fno-pie -no-pie -static -nostdlib -Wl,--build-id=none -o "$out/jet" "$here/main.c" "$out/jet-runtime/jet_runtime.pack.o"
