#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cc -std=c11 -fPIC -I"$root" -c "$root/zreader.c" -o "$root/zreader.o"
ar rcs "$root/libzreader.a" "$root/zreader.o"
rm -f "$root/zreader.o"
