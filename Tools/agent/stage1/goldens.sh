#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# Batch golden harness for a generated Jet compiler (jetc0, jetc1, ...).
#
# Every case of a list goes through the generated compiler in Runner mode (as
# Compiler/Bootstrap/Tests.rs compile_and_run_source_fixture does), the Rust
# backend builds each emitted program (one backend project and one Cargo
# target: the runtime crates build once), the program runs, and its output is
# compared with the expected streams under the tests/golden.rs rules:
#   err (<stem>.err.out)  exit 70 or 1, stderr equals it
#   out (<stem>.out)      exit 0, stdout equals it (+ <stem>.stderr.out when present)
#   none                  compile + build only
#   exit 101              never a pass
# The same cases go through the reference compiler (Rust `jet build`, by
# default --release: the rustc path), so the report separates failures that
# are the generated compiler's own from failures the reference shares. All
# cases run in one go (keep going); report.md groups the failures by cause
# (stop stage + normalized first message).
#
# usage: goldens.sh [options] <jetc|-> <list>
#   <jetc>  generated compiler binary ('-' = $KEEP/jetc0)
#   <list>  fixtures    stage1/fixtures.list: hello + the Tests.rs fixture programs
#           all         every Examples/features golden with an expected stream
#           everything  every golden (also the compile+build-only ones)
#           LISTFILE    golden-cases.mjs list format
#           SUB[,SUB]   goldens whose stem contains a SUB
# options:
#   -o DIR          output dir (default $S1/goldens/<list>-<jetc name>)
#   --batch MODE    auto (default: on when the binary knows JET_BOOTSTRAP_BATCH) | on | off
#                   on: one compiler process compiles every pending case (the compiler
#                   image is restored once); a case that kills the process is recorded
#                   and the batch resumes after it. off: one process per case
#   --ref MODE      release (default, rustc) | dev (Cranelift AOT) | none
#   --ref-jet BIN   reference jet (default $GOLDENS_REF_JET, else the release `jet`
#                   beside the retained host4 build, $L/host4.path: target-rel4/release/jet)
#   --compile-only  stop after the generated-compiler pass (no backend builds or runs)
#   --resume        keep the finished per-case results already in DIR
#   --report-only   only regenerate DIR/report.md and DIR/results.tsv
# Knobs: GOLDENS_ROW_TIMEOUT (1800 s per case in a batch), GOLDENS_FRESH_MAX (25
# suspicious batch rows rerun in a fresh process), GOLDENS_CARGO_LANE (12: GB of the
# laneB slot for each backend build), GOLDENS_RUN_TIMEOUT (120 s per program run),
# GOLDENS_REF_JOBS (1), STAGE1_JETC_MEM (20G), STAGE1_JETC_TIMEOUT (whole compiler
# process, default 4 h + 30 min per case), and the lib.sh knobs.
#
# Out: DIR/report.md (grouped causes, jetc-only vs shared, timings), DIR/results.tsv
# (one row per case), DIR/cases/<slug>/ (compile.log, receipt, program.rs,
# compile.result, build.log, stdout, stderr, diff.txt, run.result), DIR/ref/<slug>/,
# DIR/jetc/ (batch row files and whole-process logs).
# Compare two runs (e.g. --batch on vs off): goldens-report.mjs --compare DIR1 DIR2.
set -u
here=$(cd "$(dirname "$0")" && pwd)
. "$here/lib.sh"

out=""; batch=auto; ref=release; ref_jet=${GOLDENS_REF_JET:-}
[ -n "$ref_jet" ] || { [ -r "$L/host4.path" ] && ref_jet=$(dirname "$(dirname "$(< "$L/host4.path")")")/jet; }
compile_only=0; resume=0; report_only=0; args=()
while [ $# -gt 0 ]; do
  case $1 in
    -o) out=$2; shift 2 ;;
    --batch) batch=$2; shift 2 ;;
    --ref) ref=$2; shift 2 ;;
    --ref-jet) ref_jet=$2; shift 2 ;;
    --compile-only) compile_only=1; shift ;;
    --resume) resume=1; shift ;;
    --report-only) report_only=1; shift ;;
    -h|--help) sed -n '2,46p' "$0"; exit 0 ;;
    -) args+=("$1"); shift ;;
    -*) die "unknown option $1" ;;
    *) args+=("$1"); shift ;;
  esac
done
[ ${#args[@]} = 2 ] || die "usage: goldens.sh [options] <jetc|-> <list> (--help)"
compiler=${args[0]}; list=${args[1]}
case $batch in auto|on|off) ;; *) die "--batch auto|on|off" ;; esac
case $ref in release|dev|none) ;; *) die "--ref release|dev|none" ;; esac
[ "$list" = fixtures ] && list=$STAGE1_TOOLS/fixtures.list
list_name=$(basename "$list" .list); list_name=${list_name//[^A-Za-z0-9_-]/_}
report() { node "$here/goldens-report.mjs" "$out"; }

load_keep
[ "$compiler" = - ] && compiler=$KEEP/jetc0
[ -x "$compiler" ] || die "compiler $compiler is not executable"
compiler=$(realpath "$compiler")
out=${out:-$S1/goldens/$list_name-$(basename "$(dirname "$compiler")")-$(basename "$compiler")}
mkdir -p "$out"
out=$(realpath "$out")
if [ $report_only = 1 ]; then report; exit $?; fi
repo=${STAGE1_SOURCE_REPO:-$KEEP_PIN}
if [ $resume = 0 ]; then rm -rf "$out/cases" "$out/ref" "$out/jetc" "$out/backend"; fi
mkdir -p "$out/cases" "$out/ref" "$out/jetc"
node "$here/golden-cases.mjs" "$repo" "$out" "$list" > "$out/staging.log" 2>&1 || { cat "$out/staging.log" >&2; die "golden staging failed"; }
total=$(wc -l < "$out/cases.tsv")
[ "$total" -gt 0 ] || die "no cases selected by '$list'"
if [ $batch = auto ]; then
  if grep -qaF JET_BOOTSTRAP_BATCH "$compiler"; then batch=on; else batch=off; fi
fi
{ echo "compiler=$compiler"; echo "compiler_mtime=$(stat -c %y "$compiler")"; echo "list=$list"; echo "cases=$total"
  echo "batch=$batch"; echo "ref=$ref"; echo "ref_jet=$ref_jet"; echo "repo=$repo"; echo "started=$(date -Is)"; } > "$out/run.env"
note "goldens: $total case(s), compiler $compiler (batch $batch), reference $ref, out $out"

slug_of() { local s=${1//[^A-Za-z0-9_-]/_}; echo "$s"; }
# result FILE KEY=VALUE...: a per-case key=value file (values are one line).
result() { local file=$1; shift; printf '%s\n' "$@" | tr -d '\r' > "$file"; }
# first_panic LOG: the message of the first panic in a compiler log.
first_panic() { awk '/panicked at /{getline; print; exit}' "$1" | cut -c1-400; }
# kill_tree PID: SIGKILL every descendant of PID (a compiler whose exe is not
# the binary itself, e.g. the stand-in's node).
kill_tree() { local child; for child in $(pgrep -P "$1"); do kill_tree "$child"; kill -KILL "$child" 2> /dev/null; done; }

export STAGE1_JETC_MEM=${STAGE1_JETC_MEM:-20G} STAGE1_SAMPLE_SECS=${STAGE1_SAMPLE_SECS:-600}
row_timeout=${GOLDENS_ROW_TIMEOUT:-1800}

# compile_verdict DIR LOG ENDED MODE SECS: classifies one compiled case into DIR/compile.result.
# ENDED: ok | panicked (the harness caught a panic) | crash:<how the process ended>.
compile_verdict() {
  local dir=$1 log=$2 ended=$3 mode=$4 secs=$5 verdict message codes=""
  if [ "$ended" = ok ] && receipt_ok "$dir/receipt" runner && [ -s "$dir/program.rs" ]; then
    verdict=ok; message=""
  elif [ "$ended" = ok ] && [ -f "$dir/receipt" ]; then
    verdict=incomplete
    message=$(receipt_reports "$dir/receipt" | awk -F'\t' '$1 ~ /^E/ {print $1 " " $3; exit}')
    [ -n "$message" ] || message=$(receipt_reports "$dir/receipt" | awk -F'\t' '{print $1 " " $3; exit}')
    [ -n "$message" ] || message="complete=$(sed -n 's/^complete=//p' "$dir/receipt") source_bytes=$(sed -n 's/^source_bytes=//p' "$dir/receipt"), no reports"
    codes=$(receipt_reports "$dir/receipt" | cut -f1 | grep -E '^E[0-9]+$' | sort -u | tr '\n' ' ')
  elif [ "$ended" = ok ]; then
    verdict=no-receipt; message="the compiler finished without writing a receipt"
  elif [ "$ended" = panicked ]; then
    verdict=panic; message=$(first_panic "$log")
  else
    verdict=crash; message="${ended#crash:}"
    grep -q 'panicked at ' "$log" 2> /dev/null && message="$message: $(first_panic "$log")"
  fi
  result "$dir/compile.result" "verdict=$verdict" "mode=$mode" "secs=$secs" "message=${message//$'\n'/ }" "codes=${codes% }"
}

# ---- Pass 1: the generated compiler ----------------------------------------
# One process per case (batch off), or batches of every pending case that
# resume after a case that ended the process (batch on).
compile_fresh() {
  local stem=$1 root=$2 entry=$3 dir rc start ended
  dir=$out/cases/$(slug_of "$stem")
  mkdir -p "$dir"; rm -f "$dir/program.rs" "$dir/receipt"
  start=$SECONDS
  STAGE1_JETC_TIMEOUT=${STAGE1_JETC_TIMEOUT:-$(( 3600 + row_timeout ))} \
    jetc "$compiler" runner "$root" "$entry" "$dir/program.rs" "$dir/receipt" "$dir/compile.log" < /dev/null
  rc=$?
  case $rc in
    0) ended=ok ;;
    101) ended=panicked ;;
    *) ended="crash:$(cat "$dir/compile.log.verdict" 2> /dev/null)" ;;
  esac
  compile_verdict "$dir" "$dir/compile.log" "$ended" fresh $(( SECONDS - start ))
}

compile_batches() {
  local iter=0 rows stems n rc jpid pid last cur since killed log verdict i stem dir ended started secs why
  while :; do
    rows=$out/jetc/batch-$(( iter + 1 )).rows; stems=${rows%.rows}.stems
    : > "$rows"; : > "$stems"
    while IFS=$'\t' read -r stem root entry _; do
      dir=$out/cases/$(slug_of "$stem")
      [ -f "$dir/compile.result" ] && continue
      mkdir -p "$dir"; rm -f "$dir/program.rs" "$dir/receipt"
      printf '%s\t%s\t%s\t%s\n' "$root" "$entry" "$dir/program.rs" "$dir/receipt" >> "$rows"
      echo "$stem" >> "$stems"
    done < "$out/cases.tsv"
    n=$(wc -l < "$rows")
    [ "$n" = 0 ] && { rm -f "$rows" "$stems"; break; }
    iter=$(( iter + 1 )); log=${rows%.rows}.log
    note "goldens: batch $iter: $n case(s)"
    STAGE1_JETC_BATCH=$rows STAGE1_JETC_TIMEOUT=${STAGE1_JETC_TIMEOUT:-$(( 3600 + n * row_timeout ))} \
      jetc "$compiler" runner "$out/jetc" batch "${rows%.rows}.rs" "${rows%.rows}.receipt" "$log" < /dev/null &
    jpid=$!
    # Per-case timeout: the newest begin marker without its end marker.
    # Startup (image restore) time: launch to the first begin marker (5 s steps).
    cur=""; since=$SECONDS; killed=""; launched=$SECONDS
    while kill -0 "$jpid" 2> /dev/null; do
      sleep 5
      last=$(grep -aoE '^jet-bootstrap-batch: (begin|end) [0-9]+' "$log" 2> /dev/null | tail -n 1)
      case $last in
        *begin*) i=${last##* }
                 [ -f "${rows%.rows}.startup_secs" ] || echo $(( SECONDS - launched )) > "${rows%.rows}.startup_secs"
                 [ "$i" != "$cur" ] && { cur=$i; since=$SECONDS; }
                 if [ -z "$killed" ] && [ $(( SECONDS - since )) -gt "$row_timeout" ]; then
                   killed=$cur
                   pid=$(descendant_with_exe "$jpid" "$compiler") && kill -KILL "$pid" || kill_tree "$jpid"
                 fi ;;
        *) cur="" ;;
      esac
    done
    wait "$jpid"; rc=$?
    verdict=$(cat "$log.verdict" 2> /dev/null || echo "exited $rc")
    # Split the process log into per-case logs at the markers; the text outside
    # every case (image restore, startup) goes to <batch>.startup.
    rm -f "${rows%.rows}".case-* "${rows%.rows}".ended-* "${rows%.rows}.startup"
    awk -v prefix="${rows%.rows}" '
      /^jet-bootstrap-batch: begin [0-9]+/ { file = prefix ".case-" $3; printf "" > file; next }
      /^jet-bootstrap-batch: end [0-9]+/ { e = prefix ".ended-" $3; print $4, $5 > e; close(e); if (file) close(file); file = ""; next }
      { if (file) print >> file; else print >> (prefix ".startup") }
    ' "$log"
    started=0; i=0
    while read -r stem; do
      dir=$out/cases/$(slug_of "$stem")
      if [ -f "${rows%.rows}.case-$i" ]; then
        started=$(( started + 1 ))
        mv "${rows%.rows}.case-$i" "$dir/compile.log"
        secs=""
        if [ -f "${rows%.rows}.ended-$i" ]; then
          read -r ended secs < "${rows%.rows}.ended-$i"; rm -f "${rows%.rows}.ended-$i"
          secs=${secs%ms}; [ -n "$secs" ] && secs=$(( secs / 1000 ))
        elif [ "$killed" = "$i" ]; then
          ended="crash:TIMEOUT (case over ${row_timeout}s in batch $iter)"
        else
          ended="crash:$verdict (batch $iter)"
        fi
        compile_verdict "$dir" "$dir/compile.log" "$ended" "batch$iter" "$secs"
        echo "batch=$iter index=$i" >> "$dir/compile.result"
      fi
      i=$(( i + 1 ))
    done < "$stems"
    if [ "$started" = 0 ]; then
      # The process ended before its first case: no batch can make progress.
      why="startup: $verdict"; grep -q 'panicked at ' "${rows%.rows}.startup" 2> /dev/null && why="$why: $(first_panic "${rows%.rows}.startup")"
      while read -r stem; do
        dir=$out/cases/$(slug_of "$stem")
        cp "${rows%.rows}.startup" "$dir/compile.log" 2> /dev/null || : > "$dir/compile.log"
        result "$dir/compile.result" "verdict=crash" "mode=batch$iter" "secs=" "message=${why//$'\n'/ }"
      done < "$stems"
      break
    fi
  done
}

# A batch row whose failure may come from state an earlier row left behind
# (a crash, or any failure after a panic or crash earlier in the same process)
# is rerun alone; the fresh result becomes the verdict (mode=fresh,
# batch_verdict=...) and the batch one stays in compile.batch.{result,log}.
rerun_suspicious() {
  local max=${GOLDENS_FRESH_MAX:-25} count=0 stem root entry dir v batch_of
  local -A tainted=()
  while IFS=$'\t' read -r stem root entry _; do
    dir=$out/cases/$(slug_of "$stem")
    v=$(sed -n 's/^verdict=//p' "$dir/compile.result")
    batch_of=$(sed -n 's/^batch=\([0-9]*\).*/\1/p' "$dir/compile.result")
    [ -n "$batch_of" ] && [ "$v" != ok ] || continue
    if [ "$v" = crash ] || [ -n "${tainted[$batch_of]:-}" ]; then
      if [ "$count" -lt "$max" ]; then
        count=$(( count + 1 ))
        mv "$dir/compile.result" "$dir/compile.batch.result"; mv "$dir/compile.log" "$dir/compile.batch.log"
        note "goldens: fresh rerun of $stem (batch verdict $v)"
        compile_fresh "$stem" "$root" "$entry"
        echo "batch_verdict=$v" >> "$dir/compile.result"
      else
        echo "suspicious=unverified (GOLDENS_FRESH_MAX=$max reached)" >> "$dir/compile.result"
      fi
    fi
    case $v in panic|crash) tainted[$batch_of]=1 ;; esac
  done < "$out/cases.tsv"
}

take_stage0_lock
if [ $batch = on ]; then
  compile_batches
  rerun_suspicious
else
  while IFS=$'\t' read -r stem root entry _; do
    [ -f "$out/cases/$(slug_of "$stem")/compile.result" ] && continue
    compile_fresh "$stem" "$root" "$entry"
  done < "$out/cases.tsv"
fi
# Builds, runs and the reference do not need the stage-zero lane.
if [ "${STAGE1_LOCKED:-0}" = 1 ] && { true >&8; } 2> /dev/null; then exec 8>&-; unset STAGE1_LOCKED; fi

# run_program BIN ROOT STDIN EXPECT EXPBASE DIR -> DIR/run.result
run_program() {
  local bin=$1 root=$2 stdin=$3 expect=$4 exp=$5 dir=$6 rc why=""
  [ "$stdin" = - ] && stdin=/dev/null
  ( cd "$root" && ulimit -c 0 && systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax=6G -p MemorySwapMax=0 \
      env NO_COLOR=1 JET_UI_HEADLESS=1 TMPDIR="$SCRATCH" timeout "${GOLDENS_RUN_TIMEOUT:-120}" "$bin" < "$stdin" > "$dir/stdout" 2> "$dir/stderr" )
  rc=$?
  if [ "$rc" = 101 ]; then why="exit 101 (panic)"
  elif [ "$rc" = 124 ]; then why="run timeout (${GOLDENS_RUN_TIMEOUT:-120}s)"
  elif [ "$expect" = err ]; then
    case $rc in 70|1) ;; *) why="exit $rc, expected 70 or 1" ;; esac
    [ -z "$why" ] && ! cmp -s "$dir/stderr" "$exp.err.out" && why="stderr differs from expected .err.out"
  elif [ "$expect" = out ]; then
    [ "$rc" != 0 ] && why="exit $rc"
    [ -z "$why" ] && ! cmp -s "$dir/stdout" "$exp.out" && why="stdout differs from expected .out"
    [ -z "$why" ] && [ -f "$exp.stderr.out" ] && ! cmp -s "$dir/stderr" "$exp.stderr.out" && why="stderr differs from expected .stderr.out"
  fi
  { [ -f "$exp.out" ] && diff "$exp.out" "$dir/stdout"; [ "$expect" = err ] && diff "$exp.err.out" "$dir/stderr"; } 2> /dev/null | head -n 60 > "$dir/diff.txt"
  [ -n "$why" ] && [ "$rc" != 0 ] && [ -s "$dir/stderr" ] && why="$why: $(grep -m1 -v '^\s*$' "$dir/stderr" | cut -c1-200)"
  if [ "$expect" = none ] && [ -z "$why" ]; then result "$dir/run.result" "verdict=built" "rc=$rc"
  elif [ -z "$why" ]; then result "$dir/run.result" "verdict=pass" "rc=$rc"
  else result "$dir/run.result" "verdict=fail" "rc=$rc" "message=${why//$'\n'/ }"; fi
}

# ---- Pass 2: backend builds and runs (serial: one backend project/target) --
if [ $compile_only = 0 ]; then
  export STAGE1_CARGO_LANE=${GOLDENS_CARGO_LANE:-12}
  while IFS=$'\t' read -r stem root entry expect stdin expbase <&3; do
    dir=$out/cases/$(slug_of "$stem")
    [ "$(sed -n 's/^verdict=//p' "$dir/compile.result")" = ok ] || continue
    [ -f "$dir/run.result" ] && continue
    start=$SECONDS
    if bin=$(build_backend "$out/backend" goldens_program "$dir/program.rs" "$dir/build.log" 2> "$dir/build.stage1.err"); then
      result "$dir/build.result" "verdict=ok" "secs=$(( SECONDS - start ))"
      run_program "$bin" "$root" "$stdin" "$expect" "$expbase" "$dir"
    else
      msg=$(grep -m1 -E '^error' "$dir/build.log" 2> /dev/null | cut -c1-300)
      [ -n "$msg" ] || msg=$(grep -m1 . "$dir/build.stage1.err" 2> /dev/null | cut -c1-300)
      result "$dir/build.result" "verdict=fail" "secs=$(( SECONDS - start ))" "errors=$(grep -c -E '^error(\[E[0-9]+\])?:' "$dir/build.log" 2> /dev/null)" "message=${msg:-backend build failed (see build.log)}"
    fi
  done 3< "$out/cases.tsv"
fi

# ---- Reference column: Rust `jet build` of the same staged project ---------
# Cached per (reference binary, mode, project bytes, stdin): a later run of the
# same corpus with the same reference only builds what changed.
ref_case() {
  local stem=$1 root=$2 entry=$3 expect=$4 stdin=$5 exp=$6 dir key cache work bin flags=() start rc msg
  dir=$out/ref/$(slug_of "$stem")
  [ -f "$dir/run.result" ] && return 0
  key=$( { echo "$ref $(stat -c '%s %Y' "$ref_jet") $ref_jet $entry"; [ "$stdin" = - ] || cat "$stdin"
           ( cd "$root" && find . -type f ! -path './.jet/build/*' ! -path './.jet/last-run/*' ! -path './.jet/receipts/*' -print0 | sort -z | xargs -0 sha256sum ); } | sha256sum | cut -c1-32)
  cache=$S1/goldens/ref-cache/$key
  if [ -f "$cache/run.result" ]; then rm -rf "$dir"; cp -a "$cache" "$dir"; echo "cached=$key" >> "$dir/run.result"; return 0; fi
  rm -rf "$dir"; mkdir -p "$dir"; work=$dir/project
  cp -a "$root" "$work"
  [ "$ref" = release ] && flags=(--release)
  start=$SECONDS
  ( cd "$work" && "$JET_AGENT_TOOLS/cache/laneB.sh" 6 env TMPDIR="$SCRATCH" JET_NIX_TMP_CLEANED=1 NO_COLOR=1 \
      timeout "${GOLDENS_REF_TIMEOUT:-1200}" "$repo/Tools/agent/jet-env" "$ref_jet" build "${flags[@]}" --color=never "$entry" < /dev/null ) > "$dir/build.log" 2>&1
  rc=$?
  bin=$(find "$work/.jet/build" -maxdepth 1 -type f -perm -u+x ! -name '*.rs' -printf '%T@ %p\n' 2> /dev/null | sort -rn | head -n 1 | cut -d' ' -f2-)
  if [ "$rc" = 0 ] && [ -n "$bin" ]; then
    result "$dir/build.result" "verdict=ok" "secs=$(( SECONDS - start ))"
    run_program "$bin" "$work" "$stdin" "$expect" "$exp" "$dir"
  else
    msg=$(grep -m1 -E '^(error|Error|Stop)' "$dir/build.log" | cut -c1-300)
    result "$dir/build.result" "verdict=fail" "secs=$(( SECONDS - start ))" "message=exit $rc: ${msg:-see build.log}"
    if [ "$expect" = err ] && [ "$rc" = 1 ]; then
      # A compile-time diagnostic golden: the build's diagnostics are the program's stderr.
      grep -v '^\[build\]' "$dir/build.log" > "$dir/stderr"
      if cmp -s "$dir/stderr" "$exp.err.out"; then result "$dir/run.result" "verdict=pass" "rc=1" "message=compile-time diagnostic"
      else diff "$exp.err.out" "$dir/stderr" | head -n 60 > "$dir/diff.txt"; result "$dir/run.result" "verdict=fail" "rc=1" "message=build diagnostics differ from expected .err.out"; fi
    else
      result "$dir/run.result" "verdict=fail-build" "rc=$rc" "message=exit $rc: ${msg:-see build.log}"
    fi
  fi
  rm -rf "$work/.jet/build"
  mkdir -p "$S1/goldens/ref-cache" && rm -rf "$cache" && cp -a "$dir" "$cache"
}
if [ "$ref" != none ] && [ $compile_only = 0 ]; then
  [ -x "$ref_jet" ] || die "reference jet $ref_jet is not executable (--ref-jet, or --ref none)"
  export -f ref_case run_program result slug_of
  export out ref ref_jet repo L S1 SCRATCH
  tr '\n' '\0' < "$out/cases.tsv" | xargs -0 -P "${GOLDENS_REF_JOBS:-1}" -n 1 bash -c 'IFS=$'"'"'\t'"'"' read -r a b c d e f <<< "$1"; ref_case "$a" "$b" "$c" "$d" "$e" "$f"' _
fi

echo "finished=$(date -Is)" >> "$out/run.env"
report
