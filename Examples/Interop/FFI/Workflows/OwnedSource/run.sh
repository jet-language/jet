#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
jet=${JET:-jet}
clang=${CLANG:-$(command -v clang)}
archiver=${AR:-$(command -v ar)}
target=${JET_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
work=$(mktemp -d "jet-ffi-owned.XXXXXX")
trap 'rm -rf "$work"' EXIT
project="$work/project"
cp -R "$root" "$project"
cd "$project"
"$jet" bind cpp native/frame/binding.hpp --target "$target" --clang "$clang" \
    --ar "$archiver" --pkg frames >"$work/bind.log" 2>&1
"$jet" bind native/frame/frame_protocol.h --pkg jet_cpp_frames --quiet \
    >"$work/c-bind.log" 2>&1
"$jet" build >"$work/build.log" 2>&1
output=$("$jet" run 2>"$work/run.err")
case "$output" in
    *20*1*50*) ;;
    *)
        printf 'owned-source: expected 20/expired-view/50, got: %s\n' "$output" >&2
        exit 1
        ;;
esac

invalid=$("$jet" run invalid-lifetime.jet 2>"$work/invalid.err")
case "$invalid" in
    *expired-view=1*) ;;
    *)
        printf 'owned-source: stale borrowed view was not rejected: %s\n' "$invalid" >&2
        cat "$work/invalid.err" >&2
        exit 1
        ;;
esac
printf '%s\n' 'owned-source: 20 1 50 (generation expiry and cleanup observed)'

