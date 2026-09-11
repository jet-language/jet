#!/usr/bin/env bash
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
jet=${JET:-jet}
work=$(mktemp -d "jet-ffi-png.XXXXXX")
trap 'rm -rf "$work"' EXIT

project="$work/project"
cp -R "$root" "$project"
cd "$project"
# Keep the checked-in base64 representation portable while making the original
# PNG input available at the exact path consumed by app.jet.
base64 --decode logo.png.b64 >"$work/logo.png"
cp "$work/logo.png" logo.png

"$jet" bind native/png_protocol.h --pkg png --overlay native/png_protocol.overlay.jet >"$work/bind.log" 2>&1
"$jet" build >"$work/build.log" 2>&1
output=$("$jet" run 2>"$work/run.err")
case "$output" in
    *1x1*) ;;
    *)
        printf 'external-package: expected 1x1, got: %s\n' "$output" >&2
        exit 1
        ;;
esac

printf 'not-a-png\n' >"$project/missing-or-corrupt.png"
if (cd "$project" && "$jet" run "$project/missing-image.jet") >"$work/failure.log" 2>&1; then
    printf '%s\n' 'external-package: corrupt PNG was accepted' >&2
    exit 1
fi
failure=$(cat "$work/failure.log")
case "$failure" in
    *png*|*PNG*|*decode*|*signature*|*cleanup*|*close*) ;;
    *)
        printf '%s\n' 'external-package: failure did not identify native decode or cleanup' >&2
        cat "$work/failure.log" >&2
        exit 1
        ;;
esac
printf '%s\n' 'external-package: 1x1 (decode failure and cleanup observed)'
