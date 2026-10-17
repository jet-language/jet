#!/usr/bin/env bash
# Self-host native probe: the whole Jet compiler (assembled unit + the
# Compiler/JetCli/run.jet `run` entry) through the native backend.
#   1. assemble the compiler unit from the worktree (private HOME, so the
#      shared ~/.cache/jet-luna/compiler-bootstrap is never touched);
#   2. build the lowering driver (assemble-lower.mjs) with `jet build --release`;
#   3. JET_DUMP_MIR the unit's checked MIR through `jet emit --rust`;
#   4. selfmird.mjs: streaming prune + convert into self.mird;
#   5. run the driver; a crash adds the function it was lowering to the skip
#      list and reruns (at most SKIP_ROUNDS times).
# Steps whose outputs exist are reused (FORCE=1 redoes everything).
# usage: run-probe.sh <outdir>
set -uo pipefail
O=${1:?outdir}
S=$HOME/.cache/jet-dev/scratch/NativeSelfHostProbe
WT=${WT:-/home/nate/Projects/Github/jet/.agent-worktrees/self-host-probe}
JET=${JET:-/mnt/jetscratch/candidates/dev-05ea86f65/jet}
PACK=${PACK:-/mnt/jetscratch/candidates/BackendO11/allparts/pk/jet_runtime.pack.o}
export JET_SCRATCH_ROOT=/mnt/jetscratch CARGO_TARGET_DIR=/mnt/jetscratch/targets/NativeSelfHostProbe CARGO_INCREMENTAL=0
mkdir -p "$O"
say() { echo "$(date +%T) $*" | tee -a "$O/probe.log"; }
cap() { local mem=$1; shift; systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax="$mem" -p MemorySwapMax=0 "$@"; }
[ "${FORCE:-0}" = 1 ] && rm -f "$O/self.mir" "$O/self.mird" "$O/lower/.jet/build/unit"
say "probe start: WT=$WT ($(git -C "$WT" rev-parse --short=9 HEAD)) JET=$JET"

# 1. Compiler unit.
mkdir -p "$O/home" "$O/unit"
"$WT/Tools/agent/jet-env" env HOME="$O/home" node "$WT/Compiler/Bootstrap/assemble.mjs" >> "$O/probe.log" 2>&1 || { say "assemble failed"; exit 1; }
B="$O/home/.cache/jet-luna/compiler-bootstrap"
{
  cat "$B/project/src/compiler.jet"
  printf '\n// [self-host probe entry: Compiler/JetCli/run.jet]\nuse core.process as selfhost_process\n\nfn run() {\n    selfhost_process.exit(jet_cli_main(selfhost_process.args()))\n}\n'
} > "$O/unit/selfhost.jet"
cp "$B/compiler.map.json" "$O/compiler.map.json"
printf 'name: "jet_selfhost"\nversion: "0.0.1"\nedition: "2028"\nauthority: {\n    holds: { allow: [Env, Exec, Exec.Args, FS, FS.Read, IO, Mem.Alloc, Time.Wait] }\n}\n' > "$O/unit/package.jet"
say "unit: $(wc -c < "$O/unit/selfhost.jet") bytes"

# 2. Lowering driver.
if [ ! -x "$O/lower/.jet/build/unit" ]; then
  "$WT/Tools/agent/jet-env" node "$S/assemble-lower.mjs" "$O/lower" "$WT" >> "$O/probe.log" 2>&1 || { say "assemble-lower failed"; exit 1; }
  t=$(date +%s)
  ( cd "$O/lower" && cap 14G prlimit --stack=1073741824:1073741824 timeout 5400 "$WT/Tools/agent/jet-env" "$JET" build --release unit.jet > build.log 2>&1 )
  rc=$?
  say "driver build rc=$rc in $(( $(date +%s) - t ))s"
  [ -x "$O/lower/.jet/build/unit" ] || { say "driver build failed: see $O/lower/build.log"; exit 1; }
fi

# 3. Checked MIR dump.
if [ ! -s "$O/self.mir" ]; then
  t=$(date +%s)
  ( cd "$O/unit" && JET_DUMP_MIR="$O/self.mir" RUST_MIN_STACK=268435456 cap 20G prlimit --stack=1073741824:1073741824 timeout 5400 "$WT/Tools/agent/jet-env" "$JET" emit --rust selfhost.jet > /dev/null 2> "$O/emit.log" )
  rc=$?
  say "dump rc=$rc in $(( $(date +%s) - t ))s: $(stat -c %s "$O/self.mir" 2>/dev/null || echo none) bytes"
  [ -s "$O/self.mir" ] || { say "no MIR dump: see $O/emit.log"; exit 1; }
fi

# 4. Streaming conversion.
if [ ! -s "$O/self.mird" ]; then
  t=$(date +%s)
  cap 16G "$WT/Tools/agent/jet-env" node --max-old-space-size=12288 "$S/selfmird.mjs" "$O/self.mir" "$O/self" "$WT" > "$O/selfmird.log" 2>&1
  rc=$?
  say "convert rc=$rc in $(( $(date +%s) - t ))s"
  [ "$rc" = 0 ] || { say "conversion failed: see $O/selfmird.log"; exit 1; }
fi

# 5. Lowering, skipping functions that crash it.
mkdir -p "$O/run"
touch "$O/run/skip.txt"
for round in $(seq 1 "${SKIP_ROUNDS:-6}"); do
  rm -f "$O/run/self".{issues,partial-issues,prepare-issues,progress,slow,stats,o,exe,object-issues,link-issues}
  t=$(date +%s)
  ( cd "$O/run" && SELFHOST_MIRD="$O/self.mird" SELFHOST_OUT="$O/run/self" SELFHOST_PACK="$PACK" SELFHOST_SKIP="$O/run/skip.txt" \
      cap 20G prlimit --stack=1073741824:1073741824 timeout "${ROUND_TIMEOUT:-5400}" "$O/lower/.jet/build/unit" > "$O/run/round$round.out" 2> "$O/run/round$round.err" )
  rc=$?
  say "lower round $round rc=$rc in $(( $(date +%s) - t ))s"
  [ -f "$O/run/self.issues" ] && break
  done_fns=$(wc -l < "$O/run/self.progress" 2>/dev/null || echo 0)
  # A timeout or a cgroup OOM kill is not the last function's fault: stop.
  if [ "$rc" = 124 ] || [ "$rc" = 137 ]; then say "lowering stopped by $([ "$rc" = 124 ] && echo timeout || echo 'OOM kill') after $done_fns started functions"; exit 1; fi
  last=$(awk -F'\t' 'END{print $NF}' "$O/run/self.progress" 2>/dev/null)
  [ -n "$last" ] || { say "driver died before lowering any function: see $O/run/round$round.err"; exit 1; }
  if awk -v n="$last" '$0 == n {f=1} END{exit !f}' "$O/run/skip.txt"; then say "crash repeats on skipped $last"; exit 1; fi
  echo "$last" >> "$O/run/skip.txt"
  say "crash while lowering $last: skipped next round"
done
say "probe done: $(wc -l < "$O/run/self.issues" 2>/dev/null || echo no) issue lines, prepare $(wc -l < "$O/run/self.prepare-issues" 2>/dev/null || echo no)"
