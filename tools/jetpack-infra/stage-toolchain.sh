#!/bin/sh
# Prepare a local dl.jet-lang.dev channel tree. Signing and publication stay
# with the release operator.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
exec node "$repo/tools/jetpack-infra/stage-toolchain.mjs" "$@"
