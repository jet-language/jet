#!/usr/bin/env bash
# proofq: mem=18G time=30M kind=build
# build-spike.sh TREE OUT [check|build|both]: the `jet ir` spike package of TREE (an export, never the
# live checkout), with the seed compiler (~/.local/share/jet-seeds/current, or SPIKE_JET_DIR) and the
# environment of the precommit census (JET_COMPILER_IDENTITY, JET_TOOLCHAIN_ROOT=TREE, 1G stack,
# keep-going report): `jet check Compiler/JetIR/Spike` (check), then
# `jet build --threads 12 Compiler/JetIR/Spike/run.jet` (build); `both` (default) builds only after a
# clean check. OUT (fresh) gets {check,build}.{out,err}, *-keep-going.tsv and install/{jet,jet-runtime}:
# the binary beside a packaged runtime (SPIKE_RUNTIME, default the m-14 install's jet-runtime).
# Exit: 0 clean; 1 check or build errors/rows; 2 setup failure.
set -uo pipefail
TREE=$(readlink -f "${1:?TREE}")
OUT=${2:?OUT}
MODE=${3:-both}
C=$(readlink -f "${SPIKE_JET_DIR:-$HOME/.local/share/jet-seeds/current}")
[ -x "$C/jet" ] && [ -f "$C/IDENTITY" ] || { echo "COMPILER MISSING: $C"; exit 2; }
[ -e "$OUT" ] && { echo "OUT EXISTS: $OUT"; exit 2; }
mkdir -p "$OUT" && OUT=$(readlink -f "$OUT")
cd "$TREE" || exit 2
export JET_COMPILER_IDENTITY="${SPIKE_JET_IDENTITY:-$(cat "$C/IDENTITY")}" JET_TOOLCHAIN_ROOT="$TREE" JET_KEEP_GOING=1
if [ "$MODE" = check ] || [ "$MODE" = both ]; then
  JET_KEEP_GOING_REPORT="$OUT/check-keep-going.tsv" prlimit --stack=1073741824:1073741824 -- \
    Tools/agent/jet-env "$C/jet" check --threads 12 Compiler/JetIR/Spike > "$OUT/check.out" 2> "$OUT/check.err"
  crc=$?
  rows=$(awk 'NR>1' "$OUT/check-keep-going.tsv" 2>/dev/null | wc -l)
  echo "check rc=$crc rows=$rows"
  awk '/^Error/{e=$0; getline; print e" "$2}' "$OUT/check.err" | sort | uniq -c | sort -rn | cut -c1-220 | head -40
  awk -F'\t' 'NR>1{print $1"\t"$3"\t"$4"\t"$5}' "$OUT/check-keep-going.tsv" 2>/dev/null | cut -c1-220 | head -40
  [ "$crc" = 0 ] || exit 1
fi
if [ "$MODE" = build ] || [ "$MODE" = both ]; then
  rm -rf Compiler/JetIR/Spike/.jet/build
  JET_KEEP_GOING_REPORT="$OUT/build-keep-going.tsv" timeout 2400 prlimit --stack=1073741824:1073741824 -- \
    Tools/agent/jet-env "$C/jet" build --threads 12 Compiler/JetIR/Spike/run.jet > "$OUT/build.out" 2> "$OUT/build.err"
  brc=$?
  rows=$(awk 'NR>1' "$OUT/build-keep-going.tsv" 2>/dev/null | wc -l)
  echo "build rc=$brc rows=$rows"
  awk -F'\t' 'NR>1{print $1"\t"$3"\t"$4}' "$OUT/build-keep-going.tsv" 2>/dev/null | sort | uniq -c | sort -rn | cut -c1-220 | head -40
  [ "$brc" = 0 ] && [ "$rows" = 0 ] || { head -c 2000 "$OUT/build.err"; exit 1; }
  mkdir -p "$OUT/install" && cp Compiler/JetIR/Spike/.jet/build/run "$OUT/install/jet" \
    && ln -s "$(readlink -f "${SPIKE_RUNTIME:-/mnt/jetscratch/discovery/b2/install-m14/jet-runtime}")" "$OUT/install/jet-runtime" || exit 2
  echo "install=$OUT/install/jet"
fi
exit 0
