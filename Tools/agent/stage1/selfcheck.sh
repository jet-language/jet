#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# Stage 1b: a generated Jet compiler (default: the retained jetc0) compiles the
# Jet compiler's own source, the assembled Compiler/ unit, and reports its
# diagnostics mapped back to Compiler/<file>.jet:<line>.
#
# usage: selfcheck.sh [-c COMPILER] [-s SOURCE_REPO] [-o OUTDIR] [-m factory|runner] [-b]
#   -c  generated compiler binary (default $KEEP/jetc0)
#   -s  tree whose Compiler/ is assembled (default: the live checkout; use
#       ~/.cache/jet-dev/unitcheck/good-src for the last clean unit, or the
#       retained stage-zero repo for jetc0's own input)
#   -o  output dir (default ~/.cache/jet-dev/stage1/selfcheck-<compiler name>)
#   -m  factory (default; compile + emit) or runner (also packages Host/Runner;
#       its output is the stage-one source fixedpoint.sh builds)
#   -b  (implies -m runner) after a clean compile, build the emitted compiler
#       (OUTDIR/bin/<next>: jetc0 -> jetc1) with the harness backend
#       (STAGE1_CARGO_PROFILE) and run goldens.sh fixtures (hello + the Tests.rs
#       programs) through it: OUTDIR/fixtures/report.md (reference column:
#       STAGE1_FIXTURES_REF, goldens.sh --ref, default release)
# Results: OUTDIR/receipt, OUTDIR/reports.tsv (CODE, Compiler/<file>:<line>,
# message), OUTDIR/summary.txt, OUTDIR/compile.log.
# Before selfchecking, a full source ladder must pass gates/growth-gate.sh.
# L5 profiling is separate evidence in OUTDIR/phase-profile; deferred during
# Main's frontend hold, and never a candidate-build failure.
# Exit: 0 complete=true and no E-code report (with -b: the build succeeded and
# every fixture passed), 1 otherwise, 2 unavailable.
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

compiler=""; source_repo=$JET_REPO; out=""; mode=factory; build=0
while [ $# -gt 0 ]; do
  case $1 in
    -c) compiler=$2; shift 2 ;;
    -s) source_repo=$2; shift 2 ;;
    -o) out=$2; shift 2 ;;
    -m) mode=$2; shift 2 ;;
    -b) build=1; mode=runner; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) die "unknown argument $1" ;;
  esac
done
case $mode in factory|runner) ;; *) die "mode must be factory or runner" ;; esac
load_keep
compiler=$(realpath "${compiler:-$KEEP/jetc0}")
[ -x "$compiler" ] || die "compiler $compiler is not executable"
out=$(realpath -m "${out:-$S1/selfcheck-$(basename "$compiler")}")
take_stage0_lock
mkdir -p "$out"
assemble_compiler "$source_repo" "$out" false || die "assemble failed: $(tail -3 "$out/assemble.log")"
LADDER_RUNGS=$out/rungs "$here/ladder.sh" -s "$source_repo" -m "$mode" "$compiler" "$out/ladder" > "$out/ladder.log" 2>&1
ladder_rc=$?
profile_rc=0
if [ -e "$HOME/.cache/jet-dev/scratch/EndToEndQA/frontend-hold" ]; then
  echo "profile deferred: frontend-hold present; run L5 after OPEN" > "$out/phase-profile.log"
  echo "profile deferred"
else
  PHASE_PROFILE_OUT=$out/phase-profile "$here/gates/phase-profile.sh" "$compiler" "$out/rungs/L5/project" > "$out/phase-profile.log" 2>&1
  profile_rc=$?
fi
"$here/gates/growth-gate.sh" "$out/ladder" > "$out/growth-gate.log" 2>&1
growth_rc=$?
echo "selfcheck gates: ladder=$ladder_rc growth=$growth_rc profile=$profile_rc (evidence $out)"
[ "$ladder_rc" = 0 ] && [ "$growth_rc" = 0 ] || exit 1
started=$(date +%s)
jetc "$compiler" "$mode" "$out/project" src/compiler.jet "$out/compiler.$mode.rs" "$out/receipt" "$out/compile.log"
rc=$?
secs=$(( $(date +%s) - started ))
[ -f "$out/receipt" ] && receipt_reports "$out/receipt" "$out/compiler.map.json" > "$out/reports.tsv" || : > "$out/reports.tsv"
complete=$(grep -m1 '^complete=' "$out/receipt" 2>/dev/null | cut -d= -f2)
errors=$(cut -f1 "$out/reports.tsv" | grep -c '^E')
{
  echo "selfcheck: compiler $compiler mode $mode source $source_repo"
  echo "rc=$rc complete=${complete:-no-receipt} reports=$(wc -l < "$out/reports.tsv") errors=$errors secs=$secs"
  echo "run: $(cat "$out/compile.log.verdict" 2> /dev/null)"
  [ -f "$out/receipt" ] || { echo "no receipt; last log lines:"; tail -5 "$out/compile.log"; }
  cut -f1 "$out/reports.tsv" | sort | uniq -c | sort -rn | head -20
  head -40 "$out/reports.tsv"
} | tee "$out/summary.txt"
[ "$rc" = 0 ] && [ "$complete" = true ] && [ "$errors" = 0 ] || exit 1
[ "$build" = 1 ] || exit 0

# -b: the emitted compiler -> binary -> fixtures (hello first).
case $(basename "$compiler") in jetc[0-9]) next=jetc$(( ${compiler##*jetc} + 1 )) ;; *) next=$(basename "$compiler")-next ;; esac
with_artifact_main "$out/compiler.runner.rs" "$out/$next.rs"
mkdir -p "$out/bin"
started=$(date +%s)
if ! bin=$(build_backend "$out/backend-$next" "$next" "$out/$next.rs" "$out/$next.build.log"); then
  echo "build: $next FAILED in $(( $(date +%s) - started ))s: $(grep -m1 -E '^error' "$out/$next.build.log" | cut -c1-300) (log $out/$next.build.log)" | tee -a "$out/summary.txt"
  exit 1
fi
cp "$bin" "$out/bin/$next"
echo "build: $next built in $(( $(date +%s) - started ))s (${STAGE1_CARGO_PROFILE:-dev}) -> $out/bin/$next" | tee -a "$out/summary.txt"
started=$(date +%s)
"$here/goldens.sh" --ref "${STAGE1_FIXTURES_REF:-release}" -o "$out/fixtures" "$out/bin/$next" fixtures > "$out/fixtures.log" 2>&1
grc=$?
echo "fixtures: $next rc=$grc in $(( $(date +%s) - started ))s (report $out/fixtures/report.md)" | tee -a "$out/summary.txt"
[ "$grc" = 0 ] && exit 0
exit 1
