#!/usr/bin/env bash
# SP05b (#4530/#4533): whole-compiler isocheck of .agent-worktrees/sp05b, the
# jet-driver Trace counter unit tests, then a 75 s cand41f/jetc0 L5 baseline
# profile (MemoryMax=10G) for cost attribution.
cd $HOME/Projects/Github/jet/.agent-worktrees/sp05b || exit 1
flock -o "$HOME/.cache/jet-dev/iso-priority.lock" env ISOCHECK_BASE=$HOME/Projects/Github/jet/.agent-worktrees/sp05b "$HOME/.cache/jet-dev/isocheck-priority.sh" SP05b
iso=$?
echo "== isocheck rc=$iso"
LANES_MEM=8G bash Tools/agent/cache/laneS.sh cargo test -p jet-driver --lib Trace
tests=$?
echo "== cargo test jet-driver Trace rc=$tests"
"$HOME/.cache/jet-dev/scratch/SP05b/prof-head.sh" "$HOME/.cache/jet-dev/cand41f/jetc0" "$HOME/.cache/jet-dev/scratch/PhaseProf/rungs/L5/project" "$HOME/.cache/jet-dev/scratch/SP05b/L5c41" 75 499
echo "== profile rc=$?"
[ "$iso" -eq 0 ] && [ "$tests" -eq 0 ]
