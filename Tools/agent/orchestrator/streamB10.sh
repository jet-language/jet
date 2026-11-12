#!/usr/bin/env bash
# streamB10.sh A_BATCH B_BATCH SUB... : as soon as A_BATCH's census has a stage-1 compiler with its runtime, run
# B_BATCH (the SUBs that need a seed carrying A's rows) through integrate10.sh with that compiler as its seed.
# The original of gauntlet tool entry streamb10; StreamB10.jet beside it is its Jet twin and the program in use, and
# this file stays as the entry's reference. The batches live in
# $JET_SCRATCH_ROOT/lanes/Main/integ and the seed's IDENTITY and jetpack come from $JET_SEED (Tools/agent/roots.sh);
# integrate10.sh is the one in $JET_ORCHESTRATOR_DIR, this directory unless set.
set -u
A=$1; B=$2; shift 2
. "$(dirname "$0")/../roots.sh"
M=$JET_SCRATCH_ROOT/lanes/Main; G=${JET_ORCHESTRATOR_DIR:-$(dirname "$0")}
CO=$M/integ/$A/census.out
until awk '/^runtime rc=0/{f=1} END{exit !f}' "$CO" 2>/dev/null; do
  awk '/^runtime rc=1|^build rc=[1-9]|^check rc=[1-9]/{f=1} END{exit !f}' "$CO" 2>/dev/null && { echo "streamB: $A has no compiler"; exit 2; }
  sleep 30
done
S=$M/integ/seed-$A; rm -rf "$S"; mkdir -p "$S"
cp "$M/integ/$A/tree/Compiler/JetCli/.jet/build/jet" "$S/jet"
cp -rL "$M/integ/$A/tree/Compiler/JetCli/.jet/build/jet-runtime" "$S/jet-runtime"
C0=$(readlink -f "$JET_SEED")
cp "$C0/IDENTITY" "$S/IDENTITY"; ln -sfn "$C0/jetpack" "$S/jetpack"
echo "streamB: seed $S ready"
subs=(); for s in "$@"; do if [ -d "$s" ]; then subs+=("$s"); else echo "streamB: not staged, skipped: $s"; fi; done
CENSUS_JET_DIR=$S "$G/integrate10.sh" "$B" "${subs[@]}"
