#!/bin/sh
# Stage or publish local index.jet-lang.dev and cache.jet-lang.dev trees.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
exec node "$repo/tools/jetpack-infra/stage-index-cache.mjs" "$@"
