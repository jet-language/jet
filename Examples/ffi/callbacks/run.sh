#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
jet=${JET:-jet}
work=$(mktemp -d "jet-ffi-callbacks.XXXXXX")
trap 'rm -rf "$work"' EXIT

cp -R "$root" "$work/project"
cd "$work/project"
./native/build.sh
"$jet" bind native/callbacks.h --pkg source --quiet >"$work/bind.log" 2>&1
"$jet" build >"$work/build.log" 2>&1
output=$("$jet" run 2>"$work/run.err")
expected=$'byte: 7\nstopped'
if [[ "$output" != "$expected" ]]; then
    printf 'ffi-callbacks: expected %q, got %q\n' "$expected" "$output" >&2
    cat "$work/run.err" >&2
    exit 1
fi
printf '%s\n' 'ffi-callbacks: byte: 7 then stopped (registration, event stop, ACK drain, unsubscribe)'
