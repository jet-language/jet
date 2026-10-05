#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# Fast candidate: chain-cand.sh's jetc0 (live Compiler/, host unchanged, no
# self-compile) built with fast-loop.sh's bug-discovery backend profile.
# A complete ladder and HARD growth verdict are required; profiling is retained
# separately and deferred during Main's frontend hold.
# Outputs as chain-cand.sh: keep dir $L/<name> (jetc0, phases.tsv,
# backend-units.tsv, logs), driver log $L/stage1/run-<name>/driver.log.
# The release/timing build stays chain-cand.sh (loop7.sh -> jetc0-loop.sh).
# usage: fast-cand.sh <name> [jobs]   (env: FAST_OPT, FAST_BACKEND_MEM,
#        FAST_INCREMENTAL, JETC0_LOOP_MEM; see fast-loop.sh)
set -u
L=$HOME/.cache/jet-dev
NAME=${1:?name}
JOBS=${2:-4}
OUT=$L/stage1/run-$NAME
KEEP=$L/$NAME
OV=$JET_REPO/.agent-worktrees/rel-overlay3
HOST=$(< "$L/host3.path")
mkdir -p "$OUT" "$KEEP"
echo "$(date +%T) fast loop start jobs=$JOBS opt=${FAST_OPT:-1} keep=$KEEP host=$HOST" > "$OUT/driver.log"
# As loop7.sh: this loop writes only its own keep dir, so it bypasses
# stage0.lock (FAST_TAKE_LOCK=1 restores it).
[ "${FAST_TAKE_LOCK:-}" = 1 ] || export STAGE1_LOCKED=1
JETC0_LOOP_DIR=$KEEP JETC0_LOOP_JOBS=$JOBS JETC0_LOOP_OVERLAY=$OV JETC0_LOOP_HOST=$HOST \
  "$STAGE1_TOOLS/fast-loop.sh" > "$OUT/loop.log" 2>&1
echo "$(date +%T) loop rc=$?" >> "$OUT/driver.log"
grep -q ': ok (' "$OUT/loop.log" || { echo "$(date +%T) loop not ok: stop" >> "$OUT/driver.log"; exit 1; }
echo "$(date +%T) jetc0 ready (fast profile); ladder starts" >> "$OUT/driver.log"
LADDER_RUNGS=$OUT/rungs "$STAGE1_TOOLS/ladder.sh" "$KEEP/jetc0" "$OUT/ladder" > "$OUT/ladder.log" 2>&1
ladder_rc=$?
# Profiling is a separate acceptance receipt, never a candidate-build failure.
# Main's own hold remains present during a candidate build; profile L5 after OPEN.
profile_rc=0
if [ -e "$HOME/.cache/jet-dev/scratch/EndToEndQA/frontend-hold" ]; then
  echo "profile deferred: frontend-hold present; run L5 after OPEN" > "$OUT/phase-profile.log"
  echo "$(date +%T) profile deferred" >> "$OUT/driver.log"
else
  PHASE_PROFILE_OUT=$KEEP/phase-profile "$STAGE1_TOOLS/gates/phase-profile.sh" "$KEEP/jetc0" "$OUT/rungs/L5/project" > "$OUT/phase-profile.log" 2>&1
  profile_rc=$?
fi
"$STAGE1_TOOLS/gates/growth-gate.sh" "$OUT/ladder" > "$OUT/growth-gate.log" 2>&1
growth_rc=$?
echo "$(date +%T) ladder rc=$ladder_rc growth rc=$growth_rc profile rc=$profile_rc" >> "$OUT/driver.log"
[ "$ladder_rc" = 0 ] && [ "$growth_rc" = 0 ] || exit 1
