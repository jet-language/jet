#!/usr/bin/env bash
# A stand-in for runtime-pack-check.sh SELF TREE OUT ID: packs into OUT/out
# unless STANDIN_PACK_RC fails it.
echo "runtime-pack-check: $4 packs with $1"
if [ "${STANDIN_PACK_RC:-0}" != 0 ]; then echo "runtime rc=$STANDIN_PACK_RC (packager)"; exit "$STANDIN_PACK_RC"; fi
mkdir -p "$3/out" && : > "$3/out/native.key"
echo "runtime rc=0 (packager)"
