#!/usr/bin/env bash
# tk-build.sh TREE JETDIR
# Print the path of a testkit binary built from TREE/Tools/testkit (the kit CLI over the
# TREE/Compiler/JetTestKit engine) by JETDIR/jet, so a suite run uses the testkit of the tree
# under test, not a stale prebuilt one.
# Cache: $JET_STATE_ROOT/scratch/TestGreen/tk/<key>/testkit, where <key> hashes every
# file under TREE/Tools/testkit and TREE/Compiler/JetTestKit (outside .jet/) plus the jet
# binary; a hit returns at once.
# Builds take one lock (tk/.build.lock): a build writes TREE/Tools/testkit/.jet/build/run,
# so two callers on the same tree must not build at once, and a caller that waited on the
# lock reuses the binary the first caller published. The compiler identity is
# JETDIR/IDENTITY when present, else the latest published base's. Build log: tk/<key>.log.
set -euo pipefail
[ "$#" -eq 2 ] || { echo "usage: tk-build.sh TREE JETDIR" >&2; exit 64; }
tree=$(cd "$1" && pwd)
jetdir=$(cd "$2" && pwd)
jet=$jetdir/jet
[ -x "$jet" ] || { echo "tk-build: no executable jet at $jet" >&2; exit 64; }
[ -f "$tree/Tools/testkit/package.jet" ] || { echo "tk-build: no Tools/testkit package under $tree" >&2; exit 64; }
. "$(dirname "${BASH_SOURCE[0]}")/../roots.sh"
cache=$JET_STATE_ROOT/scratch/TestGreen/tk
mkdir -p "$cache"
key=$(
  cd "$tree"
  { find Tools/testkit Compiler/JetTestKit -name .jet -prune -o -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum
    sha256sum "$jet" | awk '{print $1 "  <jet>"}'
  } | sha256sum | cut -c1-20
)
bin=$cache/$key/testkit
if [ -x "$bin" ]; then echo "$bin"; exit 0; fi
identity_file=$jetdir/IDENTITY
[ -f "$identity_file" ] || identity_file=$JET_BASE_ROOT/latest/IDENTITY
identity=$(awk 1 "$identity_file")
exec 9>"$cache/.build.lock"
flock 9
if [ -x "$bin" ]; then echo "$bin"; exit 0; fi
log=$cache/$key.log
built=$tree/Tools/testkit/.jet/build/run
rm -f "$built"
scratch=$JET_STATE_ROOT/scratch/TestGreen/tmp
mkdir -p "$scratch"
if ! ( cd "$tree" && unset JET JET_TOOLCHAIN_ROOT &&
       JET_TOOLCHAIN_ROOT=$tree JET_COMPILER_IDENTITY=$identity TMPDIR=$scratch NO_COLOR=1 \
       timeout 1800 "$jet" build Tools/testkit ) > "$log" 2>&1; then
  echo "tk-build: testkit build failed; see $log" >&2
  exit 1
fi
[ -x "$built" ] || { echo "tk-build: build reported success but $built is missing; see $log" >&2; exit 1; }
mkdir -p "$cache/$key"
cp "$built" "$cache/$key/.testkit.tmp"
{ echo "tree: $tree"; echo "jet: $jet"; echo "identity: $identity"; echo "built: $(date -Is)"; } > "$cache/$key/receipt"
mv "$cache/$key/.testkit.tmp" "$bin"
echo "$bin"
