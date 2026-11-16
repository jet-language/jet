#!/usr/bin/env bash
# Rebuilds the core-activation entry's stand-in binaries from arm.c: bin/small,
# bin/big (more functions) and bin/failing (exits 3).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out="$here/../bin"
mkdir -p "$out"
flags=(-O2 -fno-pie -no-pie -static -nostdlib -fno-stack-protector -Wl,--build-id=none)
cc "${flags[@]}" -o "$out/small" "$here/arm.c"
cc "${flags[@]}" -DBIG -o "$out/big" "$here/arm.c"
cc "${flags[@]}" -DEXIT=3 -o "$out/failing" "$here/arm.c"
