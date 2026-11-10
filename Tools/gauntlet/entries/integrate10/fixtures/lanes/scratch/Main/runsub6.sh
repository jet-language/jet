#!/bin/bash
# Main's runsub6.sh, as the integrate10 fixture runs it.
# runsub6.sh MODE SUBDIR : run one SUB's test lines (MODE=new: the batch compiler on the composed tree; MODE=base:
# rerun its failing lines with the seed on the clean dev export). Env from integrate6.sh: I T B SELF C0 ID G.
set -u
MODE=$1; s=${2%/}; n=$(basename "$s")
if [ "$MODE" = new ]; then
  k=0
  while IFS= read -r line; do
    [ -n "$line" ] && [ "${line#\#}" = "$line" ] || continue
    k=$((k+1))
    # A lane check already ran in the worker's lane (CHECK OK receipt); it is not a batch proof.
    case "$line" in *LaneCheck*) printf '0\t%s\n' "$line (skipped: lane receipt)" >> "$I/tests/$n.rc"; continue;; esac
    ( cd "$T" && env PATH="$I/bin:$PATH" JET="$SELF" JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$T" \
        CAP_CPUS=0-31 timeout 1500 bash "$G/capped.sh" 10G nice -n 19 bash -c "$line" ) > "$I/tests/$n.$k.out" 2>&1
    printf '%s\t%s\n' "$?" "$line" >> "$I/tests/$n.rc"
  done < "$s/tests"
else
  rcf=$I/tests/$n.rc; k=0
  while IFS=$'\t' read -r rc line; do
    k=$((k+1)); [ "$rc" = 0 ] && continue
    ( cd "$B" && env PATH="$I/basebin:$PATH" JET="$C0/jet" JET_COMPILER_IDENTITY="$ID" JET_TOOLCHAIN_ROOT="$B" \
        CAP_CPUS=0-31 timeout 1500 bash "$G/capped.sh" 10G nice -n 19 bash -c "$line" ) > "$I/tests/$n.$k.base.out" 2>&1
    echo $? > "$I/tests/$n.$k.base.rc"
  done < "$rcf"
fi
