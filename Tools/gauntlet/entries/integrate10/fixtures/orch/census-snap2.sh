#!/usr/bin/env bash
# A stand-in for census-snap2.sh in the integrate10 fixture: it lists the files it is given and a checksum of
# the composed tree, writes the keep-going rows of rows/*.tsv and, unless STANDIN_UNBUILT is set, builds the
# batch compiler (orch/built-jet) and names the jetpack it built. Exit: STANDIN_CENSUS_RC, or with
# STANDIN_CENSUS=sum one more than the tree checksum's low byte.
echo "census of $CENSUS_TREE into $CENSUS_OUT on cpus $CAP_CPUS"
for f in "$@"; do echo "named $f"; done
sum=$(cd "$CENSUS_TREE" && find . -type f | LC_ALL=C sort | xargs cksum | cksum)
echo "tree rc=0 sum=${sum%% *}"
mkdir -p "$CENSUS_OUT"
cp rows/check.tsv "$CENSUS_OUT/check-keep-going.tsv"
cp rows/build.tsv "$CENSUS_OUT/build-keep-going.tsv"
echo "check rc=0"
echo "build rc=0 rows=4"
if [ -z "${STANDIN_UNBUILT:-}" ]; then
  mkdir -p "$CENSUS_TREE/Compiler/JetCli/.jet/build"
  cp orch/built-jet "$CENSUS_TREE/Compiler/JetCli/.jet/build/jet"
  printf 'hosted bootstrap\nhosted-binary: orch/old-jetpack\nhosted-binary: orch/built-jetpack built\n' > "$CENSUS_OUT/jetpack.log"
fi
[ "${STANDIN_CENSUS:-}" = sum ] && exit $(( ${sum%% *} % 255 + 1 ))
exit "${STANDIN_CENSUS_RC:-0}"
