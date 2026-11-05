#!/usr/bin/env bash
# publish-latest.sh : after a successful lander batch, publish the batch-verified compiler (the census s1 built from
# HEAD + the batch) and a runtime pack as $JET_SCRATCH_ROOT/discovery/b2/install-latest (atomic swap), with COMMIT and
# IDENTITY files. Lanes measure on it instead of waiting for a build slot. Roots: Tools/agent/roots.sh.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
OUT=$JET_SCRATCH_ROOT/discovery/r14/census
W=$JET_DEV_WORKTREE
D=$JET_SCRATCH_ROOT/discovery/b2
S1=$JET_SCRATCH_ROOT/discovery/r14/snap/Compiler/JetCli/.jet/build/jet
[ -x "$S1" ] || { echo "publish-latest: no census compiler"; exit 1; }
PACK=$OUT/runtime/out
[ -f "$PACK/native.key" ] || PACK=$D/install-latest/jet-runtime
[ -f "$PACK/native.key" ] || PACK=$(ls -td "$D"/install-m*/jet-runtime 2>/dev/null | head -1)
T=$D/install-latest.new.$$
rm -rf "$T" && mkdir -p "$T" && cp "$S1" "$T/jet" && cp -r "$PACK" "$T/jet-runtime" || exit 1
git -C "$W" rev-parse HEAD > "$T/COMMIT"
cat "$(readlink -f "$JET_SEED")/IDENTITY" > "$T/IDENTITY"
rm -rf "$D/install-latest.old"; [ -d "$D/install-latest" ] && mv "$D/install-latest" "$D/install-latest.old"
mv "$T" "$D/install-latest" && rm -rf "$D/install-latest.old"
echo "published install-latest at $(cat "$D/install-latest/COMMIT")"
