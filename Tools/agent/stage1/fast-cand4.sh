#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# fast-cand4.sh: fast-cand.sh on the speed-only host4 (perf/CYCLE-BUDGET.md).
# Same candidate (live Compiler/, no self-compile), same outputs and gates as
# fast-cand.sh: keep dir $L/<name> (jetc0, phases.tsv, backend-units.tsv,
# logs), driver log $L/stage1/run-<name>/driver.log, then the complete ladder
# and HARD growth verdict (the phase profile is logged evidence and never
# blocks; deferred under frontend-hold). Differences, all speed:
#   - host4 ($L/host4.path, built from .agent-worktrees/rel-overlay4 into
#     target-rel4: host3 plus speed-only patches, perf/host4-speed.patch and
#     PerfAudit-1's DiagnosticCodec origin memo), overlay rel-overlay4;
#   - fast-loop4.sh: own unit package jetc0_fast4 and target dir, smaller
#     backend shards (FAST4_SHARD_BYTES, default 4 MiB), more cargo jobs.
# usage: fast-cand4.sh <name> [jobs]   (env: FAST_OPT, FAST_BACKEND_MEM,
#        FAST_INCREMENTAL, JETC0_LOOP_MEM, FAST4_SHARD_BYTES, FAST4_COLD_BYTES;
#        see fast-loop4.sh)
set -u
L=$HOME/.cache/jet-dev
NAME=${1:?name}
JOBS=${2:-12}
OUT=$L/stage1/run-$NAME
KEEP=$L/$NAME
OV=$JET_REPO/.agent-worktrees/rel-overlay4
HOST=$(< "$L/host4.path")
mkdir -p "$OUT" "$KEEP"
echo "$(date +%T) fast loop4 start jobs=$JOBS opt=${FAST_OPT:-1} shard=${FAST4_SHARD_BYTES:-4194304} keep=$KEEP host=$HOST" > "$OUT/driver.log"
# As fast-cand.sh: this loop writes only its own keep dir, so it bypasses
# stage0.lock (FAST_TAKE_LOCK=1 restores it).
[ "${FAST_TAKE_LOCK:-}" = 1 ] || export STAGE1_LOCKED=1
JETC0_LOOP_DIR=$KEEP JETC0_LOOP_JOBS=$JOBS JETC0_LOOP_OVERLAY=$OV JETC0_LOOP_HOST=$HOST \
  "$STAGE1_TOOLS/fast-loop4.sh" > "$OUT/loop.log" 2>&1
echo "$(date +%T) loop rc=$?" >> "$OUT/driver.log"
# Per-phase timings (fast-loop4.sh report rows) into the driver log and stdout.
grep -E '^(sync|frontend|project|backend|jetc0|hello|total) ' "$OUT/loop.log" | cut -c1-240 | tee -a "$OUT/driver.log"
grep -q ': ok (' "$OUT/loop.log" || { echo "$(date +%T) loop not ok: stop" >> "$OUT/driver.log"; exit 1; }
echo "$(date +%T) jetc0 ready (fast profile); ladder starts" >> "$OUT/driver.log"
LADDER_RUNGS=$OUT/rungs "$STAGE1_TOOLS/ladder.sh" "$KEEP/jetc0" "$OUT/ladder" > "$OUT/ladder.log" 2>&1
ladder_rc=$?
# Save evidence even when the ladder/growth gate fails; never mask their
# status. The phase profile never blocks the candidate (Main): it is deferred
# while a candidate frontend holds the machine, and its status is only logged.
if [ -e "$HOME/.cache/jet-dev/scratch/EndToEndQA/frontend-hold" ]; then
  echo "$(date +%T) profile deferred (frontend-hold)" >> "$OUT/driver.log"
  profile_rc=0
else
  PHASE_PROFILE_OUT=$KEEP/phase-profile "$STAGE1_TOOLS/gates/phase-profile.sh" "$KEEP/jetc0" "$OUT/rungs/L5/project" > "$OUT/phase-profile.log" 2>&1
  profile_rc=$?
fi
"$STAGE1_TOOLS/gates/growth-gate.sh" "$OUT/ladder" > "$OUT/growth-gate.log" 2>&1
growth_rc=$?
echo "$(date +%T) ladder rc=$ladder_rc growth rc=$growth_rc profile rc=$profile_rc (profile does not block)" >> "$OUT/driver.log"
[ "$ladder_rc" = 0 ] && [ "$growth_rc" = 0 ] || exit 1
