#!/bin/sh
# Prepare site/dist/keys from externally supplied public trust files.
# This wrapper never creates or accepts private signing material.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
binary=${JETPACK_TRUST_ROOT_BIN:-"$repo/target/debug/jetpack-trust-root"}
output=${JET_TRUST_OUTPUT:-"$repo/site/dist/keys"}

[ -x "$binary" ] || {
    echo "missing $binary; build jetpack-trust-root first" >&2
    exit 1
}

exec "$binary" export --output "$output" "$@"
