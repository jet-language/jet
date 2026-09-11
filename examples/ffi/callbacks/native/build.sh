#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cc=${CC:-cc}
ar=${AR:-ar}
"$cc" -std=c11 -fPIC -I"$root" -c "$root/callbacks.c" -o "$root/callbacks.o"
"$ar" rcs "$root/libsource.a" "$root/callbacks.o"
rm -f "$root/callbacks.o"
