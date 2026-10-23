#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cc -std=c11 -fPIC -I"$root/../include" -c "$root/foreign.c" -o "$root/foreign.o"
ar rcs "$root/libselfhost_foreign.a" "$root/foreign.o"
rm -f "$root/foreign.o"
