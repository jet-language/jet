#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# The self-compile ladder: a generated compiler (jetc0, jetc1, ...) compiles
# growing real subsets of the Jet compiler, so a build is judged in minutes
# instead of one full stage-one attempt. Each rung is one aggregate unit
# assembled by the source tree's own Compiler/Bootstrap/assemble.mjs, run
# with selfcheck.sh's protocol (lib.sh `jetc`: runner mode, explicit
# environment, private store, STAGE1_JETC_MEM cap (20G), STAGE1_JETC_TIMEOUT,
# progress/stall/timeout profiles), with JET_TRACE_FILE on:
#   L1  JetFoundation                                  (+ anchor)
#   L2  + JetLexer + JetParser                         (+ anchor)
#   L3  + JetSema                                      (+ anchor)
#   L4  + JetOptimizer + JetCodegen                    (+ anchor)
#   L5  the whole compiler, selfcheck.sh's unit (Host/Entry.jet included)
# L1-L4 are libraries like the self-compile; their anchor
# (Compiler/Bootstrap/Host/LadderAnchor.jet, ladder.mjs ANCHORS) calls each
# package's public entry points the way Host/Entry.jet calls the driver.
# Rungs run serially and stop at the first failure (exit status, receipt not
# complete, or an E-code report). The report (OUT/summary.txt) has per-rung
# per-phase seconds, RSS at each phase end, and the growth table: time and
# RSS ratio rung to rung against the unit-size ratio, the fitted exponent,
# LINEAR / SUPERLINEAR / FAILED per phase, and each phase's time
# extrapolated to the full compiler.
#
# usage: ladder.sh [-s SOURCE_TREE] [-r RUNGS] [-m runner|factory] [--report] COMPILER OUT
#   -s  tree whose Compiler/ is laddered (default: COMPILER's pinned inputs,
#       <dir of COMPILER>/inputs, when present, so L5 is its own input;
#       otherwise the live checkout)
#   -r  comma-separated rungs to run, in order (default L1,L2,L3,L4,L5); rungs
#       not selected keep their earlier results in OUT and stay in the report,
#       so `-r L1` then `-r L2` with one OUT climbs step by step (same
#       COMPILER and SOURCE_TREE)
#   -m  runner (default: adds the package phase, as fixedpoint.sh's runs) or
#       factory (selfcheck.sh's default)
#   --report  only reprint OUT's report
# Rungs are (re)generated under LADDER_RUNGS (default
# ~/.cache/jet-dev/scratch/ladder). OUT/<rung>/: trace.json, compile.log
# (+ .verdict/.progress/.stall.txt), receipt, reports.tsv, result.env.
# The emitted source and image are deleted after a rung unless
# LADDER_KEEP_OUTPUT=1. Takes the stage-zero lock unless STAGE1_LOCKED=1.
# Exit: 0 every selected rung passed, 1 a rung failed, 2 unavailable.
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

source_tree=""; selected="L1,L2,L3,L4,L5"; mode=runner; report_only=0
while [ $# -gt 0 ]; do
  case $1 in
    -s) source_tree=$2; shift 2 ;;
    -r) selected=$2; shift 2 ;;
    -m) mode=$2; shift 2 ;;
    --report) report_only=1; shift ;;
    -h|--help) sed -n '2,40p' "$0"; exit 0 ;;
    -*) die "unknown argument $1" ;;
    *) break ;;
  esac
done
[ $# -eq 2 ] || die "usage: ladder.sh [-s SOURCE_TREE] [-r RUNGS] [-m runner|factory] [--report] COMPILER OUT"
case $mode in runner|factory) ;; *) die "mode must be runner or factory" ;; esac
out=$(realpath -m "$2")
if [ "$report_only" = 1 ]; then
  node "$here/ladder.mjs" report "$out" | tee "$out/summary.txt"
  exit "${PIPESTATUS[0]}"
fi
compiler=$(realpath "$1")
[ -x "$compiler" ] || die "compiler $compiler is not executable"
keep=$(dirname "$compiler")
if [ -z "$source_tree" ]; then
  if [ -f "$keep/inputs/Compiler/Bootstrap/sources.list" ]; then source_tree=$keep/inputs; else source_tree=$JET_REPO; fi
fi
source_tree=$(realpath "$source_tree")
rungs_dir=${LADDER_RUNGS:-$SCRATCH/ladder}
for rung in ${selected//,/ }; do
  case $rung in L1|L2|L3|L4|L5) ;; *) die "unknown rung $rung (L1..L5)" ;; esac
done

take_stage0_lock
mkdir -p "$out"
node "$here/ladder.mjs" rungs "$source_tree" "$rungs_dir" > "$out/rungs.log" 2>&1 || die "rung generation failed: $(tail -3 "$out/rungs.log")"
cp "$rungs_dir/rungs.tsv" "$out/rungs.tsv"
{
  echo "compiler=$compiler"
  echo "source_tree=$source_tree"
  echo "rungs_dir=$rungs_dir"
  echo "mode=$mode"
  echo "selected=$selected"
  echo "mem=${STAGE1_JETC_MEM:-20G} timeout=${STAGE1_JETC_TIMEOUT:-14400}"
  # L5 against the unit this compiler was built from (a jetc0-loop keep).
  if [ -f "$keep/compiler-project/src/compiler.jet" ]; then
    cmp -s "$keep/compiler-project/src/compiler.jet" "$rungs_dir/L5/project/src/compiler.jet" &&
      echo "L5=the compiler's own input ($keep/compiler-project)" ||
      echo "L5=differs from the compiler's own input ($keep/compiler-project)"
  fi
} > "$out/ladder.env"
note "$(grep '^L5=' "$out/ladder.env" || echo "source $source_tree")"
for rung in ${selected//,/ }; do rm -rf "${out:?}/$rung"; done

status=0
for rung in ${selected//,/ }; do
  dir=$out/$rung
  project=$rungs_dir/$rung/project
  mkdir -p "$dir"
  note "$rung: $(sed -n "s/^$rung\t\([^\t]*\)\t\([^\t]*\)\t\([^\t]*\).*/\1, \2 files, \3 bytes/p" "$out/rungs.tsv")"
  started=$(date +%s%N)
  JET_TRACE_FILE=$dir/trace.json jetc "$compiler" "$mode" "$project" src/compiler.jet "$dir/out.rs" "$dir/receipt" "$dir/compile.log"
  rc=$?
  wall_ms=$(( ($(date +%s%N) - started) / 1000000 ))
  [ -f "$dir/receipt" ] && receipt_reports "$dir/receipt" "$rungs_dir/$rung/compiler.map.json" > "$dir/reports.tsv" || : > "$dir/reports.tsv"
  complete=$(grep -m1 '^complete=' "$dir/receipt" 2> /dev/null | cut -d= -f2)
  errors=$(cut -f1 "$dir/reports.tsv" | grep -c '^E')
  output_bytes=$(stat -c %s "$dir/out.rs" 2> /dev/null || echo 0)
  if [ "$rc" = 0 ] && receipt_ok "$dir/receipt" "$mode" && [ "$errors" = 0 ]; then result=ok; else result=failed; fi
  printf 'status=%s\nrc=%s\nwall_ms=%s\ncomplete=%s\nerrors=%s\nreports=%s\noutput_bytes=%s\nverdict=%s\n' \
    "$result" "$rc" "$wall_ms" "${complete:-no-receipt}" "$errors" "$(wc -l < "$dir/reports.tsv")" "$output_bytes" "$(cat "$dir/compile.log.verdict" 2> /dev/null)" > "$dir/result.env"
  [ "${LADDER_KEEP_OUTPUT:-0}" = 1 ] || rm -f "$dir/out.rs" "$dir/out.image"
  note "$rung: $result in $(( wall_ms / 1000 ))s (rc=$rc complete=${complete:-no-receipt} errors=$errors; $(cat "$dir/compile.log.verdict" 2> /dev/null))"
  if [ "$result" != ok ]; then
    [ "$errors" = 0 ] || { grep '^E' "$dir/reports.tsv" | cut -f1 | sort | uniq -c | sort -rn | head -5; grep -m3 '^E' "$dir/reports.tsv"; } >&2
    [ -f "$dir/receipt" ] || tail -5 "$dir/compile.log" >&2
    status=1
    break
  fi
done
node "$here/ladder.mjs" report "$out" | tee "$out/summary.txt"
exit "$status"
