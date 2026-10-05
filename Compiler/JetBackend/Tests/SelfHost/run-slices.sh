#!/usr/bin/env bash
# Fallback when the whole-unit lowering of run-probe.sh dies (OOM, timeout,
# repeated crash): lower one compiler package at a time from the SAME dump.
#   1. selfmird.mjs with SELFMIRD_ROOTS=all -> self-all.mird (no entry pruning);
#   2. per package (Compiler/<Pkg>, from compiler.map.json): the decimal ids
#      of its functions -> slices/<Pkg>/roots.txt;
#   3. the run-probe.sh driver with SELFHOST_ROOTS=roots.txt, crash-skip
#      rounds as in run-probe.sh, outputs in slices/<Pkg>/run/;
#   4. rank-self.mjs per slice -> slices/<Pkg>/RANKED.tsv.
# usage: run-slices.sh <run-probe outdir> [Pkg ...]
set -uo pipefail
O=${1:?outdir}; shift
S=$HOME/.cache/jet-dev/scratch/NativeSelfHostProbe
WT=${WT:-/home/nate/Projects/Github/jet/.agent-worktrees/self-host-probe}
export JET_SCRATCH_ROOT=/mnt/jetscratch
say() { echo "$(date +%T) $*" | tee -a "$O/slices.log"; }
cap() { local mem=$1; shift; systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax="$mem" -p MemorySwapMax=0 "$@"; }
[ -s "$O/self.mir" ] && [ -x "$O/lower/.jet/build/unit" ] || { say "need $O/self.mir and the driver from run-probe.sh"; exit 1; }
if [ ! -s "$O/self-all.mird" ]; then
  SELFMIRD_ROOTS=all cap 16G "$WT/Tools/agent/jet-env" node --max-old-space-size=12288 "$S/selfmird.mjs" "$O/self.mir" "$O/self-all" "$WT" > "$O/selfmird-all.log" 2>&1 \
    || { say "all-roots conversion failed: see $O/selfmird-all.log"; exit 1; }
fi
"$WT/Tools/agent/jet-env" node -e '
const fs = require("fs");
const [O] = process.argv.slice(1);
const map = JSON.parse(fs.readFileSync(`${O}/compiler.map.json`, "utf8")).source_segments;
const sources = new Map(fs.readFileSync(`${O}/self-all.sources.tsv`, "utf8").split("\n").filter(Boolean).map((r) => r.split("\t")));
const roots = new Map();
for (const row of fs.readFileSync(`${O}/self-all.functions.tsv`, "utf8").split("\n").filter(Boolean)) {
  const [id, , , file, start] = row.split("\t");
  if (!(sources.get(file) ?? "").endsWith("selfhost.jet")) continue;
  const seg = map.find((s) => Number(start) >= s.generated.start && Number(start) < s.generated.end);
  if (!seg) continue;
  const pkg = seg.source_file.split("/")[1];
  if (!roots.has(pkg)) roots.set(pkg, []);
  roots.get(pkg).push(id);
}
for (const [pkg, ids] of roots) {
  fs.mkdirSync(`${O}/slices/${pkg}/run`, { recursive: true });
  fs.writeFileSync(`${O}/slices/${pkg}/roots.txt`, ids.join("\n") + "\n");
  console.log(`${pkg}\t${ids.length}`);
}' "$O" | tee -a "$O/slices.log"
pkgs=("$@")
[ ${#pkgs[@]} -gt 0 ] || mapfile -t pkgs < <(ls "$O/slices")
for pkg in "${pkgs[@]}"; do
  P="$O/slices/$pkg"
  for f in compiler.map.json; do ln -sfn "$O/$f" "$P/$f"; done
  ln -sfn "$O/self-all.functions.tsv" "$P/self.functions.tsv"
  ln -sfn "$O/self-all.sources.tsv" "$P/self.sources.tsv"
  touch "$P/run/skip.txt"
  for round in $(seq 1 "${SKIP_ROUNDS:-6}"); do
    rm -f "$P/run/self".{issues,partial-issues,prepare-issues,progress,slow,stats,o,exe,object-issues,link-issues}
    t=$(date +%s)
    ( cd "$P/run" && SELFHOST_MIRD="$O/self-all.mird" SELFHOST_OUT="$P/run/self" SELFHOST_ROOTS="$P/roots.txt" SELFHOST_SKIP="$P/run/skip.txt" \
        cap 20G prlimit --stack=1073741824:1073741824 timeout "${ROUND_TIMEOUT:-3600}" "$O/lower/.jet/build/unit" > "$P/run/round$round.out" 2> "$P/run/round$round.err" )
    rc=$?
    say "$pkg round $round rc=$rc in $(( $(date +%s) - t ))s"
    [ -f "$P/run/self.issues" ] && break
    if [ "$rc" = 124 ] || [ "$rc" = 137 ]; then say "$pkg: stopped by $([ "$rc" = 124 ] && echo timeout || echo 'OOM kill') after $(wc -l < "$P/run/self.progress" 2>/dev/null || echo 0) started functions"; break; fi
    last=$(awk -F'\t' 'END{print $NF}' "$P/run/self.progress" 2>/dev/null)
    [ -n "$last" ] || { say "$pkg: driver died before lowering any function"; break; }
    if awk -v n="$last" '$0 == n {f=1} END{exit !f}' "$P/run/skip.txt"; then say "$pkg: crash repeats on skipped $last"; break; fi
    echo "$last" >> "$P/run/skip.txt"
  done
  "$WT/Tools/agent/jet-env" node "$S/rank-self.mjs" "$P" "$P/RANKED.tsv" > "$P/rank.log" 2>&1 || say "$pkg: rank failed"
done
