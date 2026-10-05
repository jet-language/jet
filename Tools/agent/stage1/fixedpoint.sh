#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# Stage 1c: the retained jetc0 compiles the Jet compiler (Runner mode) and the
# Rust backend builds that source into jetc1; jetc1 compiles the same compiler
# source again, and the two emitted compiler sources must be byte-identical
# (the fixed point). Same pipeline as bootstrap_private_self_compile_harness,
# outside cargo test, with every stage retained.
#
# usage: fixedpoint.sh [-c JETC0] [-s SOURCE_REPO] [-o OUTDIR] [--stages 2|3]
#   -c  first-stage compiler (default $KEEP/jetc0; e.g. a release rebuild of the
#       retained stage-zero source: STAGE1_CARGO_PROFILE=release build_backend
#       DIR jetc0r $KEEP/stage-zero.rs LOG, which emits the same text faster)
#   -s  tree whose Compiler/ is assembled (default: the pinned copy of the
#       retained stage-zero repo, $KEEP/inputs, i.e. jetc0's own input, so
#       jetc0 and jetc1 implement the same source)
#   -o  output dir (default ~/.cache/jet-dev/stage1/fixedpoint)
#   --stages 3  also build jetc2 and compile once more; the verdict is then
#       stage2 == stage3. Use it when SOURCE_REPO is not jetc0's input (jetc1
#       then implements newer source than jetc0, so stage1 may differ).
# Results: OUTDIR/stageN.raw.rs (Runner output), OUTDIR/stageN.rs (+ artifact
# main, what the backend builds), OUTDIR/stageN.receipt, OUTDIR/bin/jetcN,
# OUTDIR/summary.txt. Exit: 0 fixed point, 1 not reached, 2 unavailable.
# Later stages build with STAGE1_CARGO_PROFILE (dev unless set).
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

first=""; source_repo=""; out=$S1/fixedpoint; stages=2
while [ $# -gt 0 ]; do
  case $1 in
    -c) first=$2; shift 2 ;;
    -s) source_repo=$2; shift 2 ;;
    -o) out=$2; shift 2 ;;
    --stages) stages=$2; shift 2 ;;
    -h|--help) sed -n '2,23p' "$0"; exit 0 ;;
    *) die "unknown argument $1" ;;
  esac
done
case $stages in 2|3) ;; *) die "--stages must be 2 or 3" ;; esac
load_keep
source_repo=${source_repo:-$KEEP_PIN}
take_stage0_lock
mkdir -p "$out/bin"
summary=$out/summary.txt
log() { echo "$(date +%T) $*" | tee -a "$summary"; }
: > "$summary"
first=$(realpath "${first:-$KEEP/jetc0}")
[ -x "$first" ] || die "first-stage compiler $first is not executable"
log "fixedpoint: first compiler $first (jetc0 id ${KEEP_ID:-?}), source $source_repo, stages $stages, pin $KEEP_PIN ($(sed -n 3p "$KEEP_PIN/.pinned"))"
# Every stage binary compiles Host/Runner, the crates and their Core texts from
# the pin (build_backend). A late pin (late_changes > 0) may differ from what
# jetc0 itself was built from; then stage1 != stage2 can come from that alone.
grep -q 'late_changes=0$' "$KEEP_PIN/.pinned" || log "WARNING: the pin was taken after the retained repo changed; use --stages 3, or rebuild jetc0 and pin right after it (pin-inputs.sh)."
check_pin() {
  pin_unchanged && return 0
  log "INVALID: the pinned inputs in $KEEP_PIN changed during this run ($1); the stages are not comparable."
  exit 2
}
# The compiler unit must match the generated main appended to each stage:
# task_roots=true keeps the task-root fixture in both.
assemble_compiler "$source_repo" "$out" "${KEEP_TASK_ROOTS:-false}" || { log "assemble FAILED: $(tail -3 "$out/assemble.log")"; exit 2; }

compiler=$first
for n in $(seq 1 "$stages"); do
  started=$(date +%s)
  jetc "$compiler" runner "$out/project" src/compiler.jet "$out/stage$n.raw.rs" "$out/stage$n.receipt" "$out/stage$n.compile.log"
  rc=$?
  if ! receipt_ok "$out/stage$n.receipt" runner || [ ! -s "$out/stage$n.raw.rs" ]; then
    log "stage $n: $(basename "$compiler") failed to compile the compiler (rc=$rc, $(grep -m1 '^complete=' "$out/stage$n.receipt" 2>/dev/null || echo no receipt)); reports:"
    [ -f "$out/stage$n.receipt" ] && receipt_reports "$out/stage$n.receipt" "$out/compiler.map.json" | head -20 | tee -a "$summary"
    tail -5 "$out/stage$n.compile.log" | tee -a "$summary"
    log "stage $n: $(cat "$out/stage$n.compile.log.verdict" 2> /dev/null)"
    exit 1
  fi
  log "stage $n: $(basename "$compiler") emitted $(wc -c < "$out/stage$n.raw.rs") bytes in $(( $(date +%s) - started ))s"
  [ "$n" = "$stages" ] && break
  with_artifact_main "$out/stage$n.raw.rs" "$out/stage$n.rs"
  started=$(date +%s)
  check_pin "before building jetc$n"
  if ! bin=$(build_backend "$out/backend-jetc$n" "jetc$n" "$out/stage$n.rs" "$out/stage$n.build.log"); then
    log "stage $n: backend build of jetc$n FAILED: $(grep -m1 -E '^error' "$out/stage$n.build.log" | cut -c1-300)"
    exit 1
  fi
  cp "$bin" "$out/bin/jetc$n"
  compiler=$out/bin/jetc$n
  log "stage $n: built $compiler in $(( $(date +%s) - started ))s"
done
check_pin "after the last stage"

a=$(( stages - 1 )); b=$stages
reports() { grep '^report_json=' "$1" | sort; }
# The compiler image travels beside the source (<raw>.image, include_bytes!):
# both halves of a stage's output must match.
same_image() {
  [ ! -e "$1" ] && [ ! -e "$2" ] && return 0
  cmp -s "$1" "$2"
}
if ! same_image "$out/stage$a.raw.image" "$out/stage$b.raw.image"; then
  log "NOT A FIXED POINT: stage$a.raw.image != stage$b.raw.image (sources $(cmp -s "$out/stage$a.raw.rs" "$out/stage$b.raw.rs" && echo identical || echo differ))"
  exit 1
fi
if cmp -s "$out/stage$a.raw.rs" "$out/stage$b.raw.rs"; then
  log "FIXED POINT: stage$a.raw.rs == stage$b.raw.rs ($(sha256sum < "$out/stage$b.raw.rs" | cut -c1-16))"
  if ! diff <(reports "$out/stage$a.receipt") <(reports "$out/stage$b.receipt") > "$out/reports.diff"; then
    log "note: identical output but the report lists differ (see $out/reports.diff)"
  fi
  exit 0
fi
diff "$out/stage$a.raw.rs" "$out/stage$b.raw.rs" > "$out/stage$a-vs-$b.diff"
log "NOT A FIXED POINT: stage$a.raw.rs ($(wc -c < "$out/stage$a.raw.rs") bytes) != stage$b.raw.rs ($(wc -c < "$out/stage$b.raw.rs") bytes); $(grep -c '^[<>]' "$out/stage$a-vs-$b.diff") differing lines, first hunk:"
head -30 "$out/stage$a-vs-$b.diff" | tee -a "$summary"
exit 1
