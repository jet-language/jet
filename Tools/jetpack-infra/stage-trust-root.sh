#!/bin/sh
# Prepare Docs/site/dist/keys from externally supplied public trust files.
# This wrapper never creates or accepts private signing material.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
binary=${JETPACK_TRUST_ROOT_BIN:?JETPACK_TRUST_ROOT_BIN must name the native jetpack-trust-root binary}
output=${JET_TRUST_OUTPUT:-"$repo/Docs/site/dist/keys"}

[ -x "$binary" ] || {
    echo "missing $binary; set JETPACK_TRUST_ROOT_BIN to the native jetpack-trust-root binary" >&2
    exit 1
}

exec "$binary" export --output "$output" "$@"
