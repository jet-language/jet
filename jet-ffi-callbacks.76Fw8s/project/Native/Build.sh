#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cc=${CC:-cc}
ar=${AR:-ar}
"$cc" -std=c11 -fPIC -I"$root" -c "$root/Callbacks.c" -o "$root/Callbacks.o"
"$ar" rcs "$root/libsource.a" "$root/Callbacks.o"
rm -f "$root/Callbacks.o"
