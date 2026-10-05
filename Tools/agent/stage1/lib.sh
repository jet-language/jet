# Shared plumbing for the stage-one scripts (sourced, not run).
#
# A generated Jet compiler (jetc0, jetc1, ...) is the Host/Runner artifact the
# stage-zero test builds (Compiler/Bootstrap/Tests.rs build_stage_zero). It has
# no CLI: it is driven by the JET_BOOTSTRAP_* environment protocol of the
# harness `main` (GENERATED_ARTIFACT_MAIN), writes Rust backend source to
# JET_BOOTSTRAP_OUTPUT and a key=value receipt (complete=, report_count=,
# report_json=...) to JET_BOOTSTRAP_RECEIPT. A Rust backend Cargo project turns
# that source into a binary; the project is written by the test's own code
# (build_backend), so it is exactly the one build_backend_artifact builds.
#
# Environment knobs (all optional):
#   JETC0_KEEP          retained stage-zero dir (default ~/.cache/jet-dev/stage0;
#                       the test fills it when run with JET_STAGE_ZERO_KEEP=<dir>)
#   STAGE1_JETC_MEM     memory cap per compiler run (default 20G: stage-one work
#                       holds the stage-zero lock, whose lane is ~21G; the Rust
#                       reference needed ~21G for the same unit)
#   STAGE1_JETC_TIMEOUT seconds per compiler run (default 14400: jetc0 is a debug
#                       build). 75 s before the deadline a 30 s perf profile of the
#                       compiler is written to <log>.perf.txt
#   STAGE1_SAMPLE_SECS  seconds between RSS + 10 s perf samples in <log>.progress
#                       (default 600; 0 disables)
#   STAGE1_PERF         perf binary (default: perf on PATH, else the nix store one)
#   STAGE1_MAIN_STACK_KIB  main-thread stack rlimit for the compiler run (default
#                       1048576 = 1 GiB; only matters for binaries whose harness
#                       main predates the 512 MiB jet-bootstrap thread)
#   STAGE1_TIER         JET_BOOTSTRAP_FACTORY_TIER (default aot)
#   STAGE1_CARGO_MEM    memory cap per backend cargo build (default 20G)
#   STAGE1_CARGO_LANE   GB (<= 12): run backend cargo builds in the shared heavy
#                       lane (laneB.sh) with that cap instead of their own scope;
#                       for program-sized builds (goldens.sh)
#   STAGE1_JETC_BATCH   a JET_BOOTSTRAP_BATCH row file passed to the compiler run
#                       (the harness main compiles every row in one process)
#   STAGE1_CARGO_PROFILE dev (default, as the stage-zero test) | release: the
#                       stage-zero test's JET_STAGE_ZERO_PROFILE=release build
#                       (profile jet-stage-release, target-stage1/jet-stage-release:
#                       opt-level 2, cold shards opt-level 0, frame pointers). Stage
#                       texts must not depend on it (a difference is a determinism
#                       bug); run time does (a debug jetc is several times slower)
#   STAGE1_CARGO_JOBS   CARGO_BUILD_JOBS for backend builds (default 4)
#   STAGE1_HOST_TEST    the jet lib test binary with the bootstrap host (default:
#                       the `host=` binary of $KEEP/stage-zero.stamp); its ignored
#                       bootstrap_backend_project_from_env test writes backend projects
#   STAGE1_BACKEND      cargo (default) | standin (dry-run: the "source" is a script)
#   STAGE1_LOCKED=1     caller already holds ~/.cache/jet-dev/stage0.lock
#
# A compiler run sees only an explicit environment (env -i, see `jetc`): HOME,
# PATH, TMPDIR, LC_ALL=C, the JET_BOOTSTRAP_* protocol, a private empty
# JET_STORE_DIR per run, RUST_BACKTRACE and RUST_MIN_STACK. Nothing from the
# caller's shell (JET_ADAPTER_*, JET_DEBUG_*, a shared record store) can make
# two stages compile differently. Core sources are compiled into the binary
# (jet_sema CORE_SOURCE_TEXTS), so no JET_TOOLCHAIN_ROOT is read.

JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
L=$HOME/.cache/jet-dev
S1=$L/stage1
KEEP=${JETC0_KEEP:-$L/stage0}
SCRATCH=$HOME/.cache/jet-dev/scratch
mkdir -p "$SCRATCH"

die() { echo "stage1: $*" >&2; exit 2; }
note() { echo "stage1: $*" >&2; }

# Stage-one work is stage-zero-lane work (~/.cache/jet-dev/wave-worker-context.md
# "Two lanes"): serialize with the stage-zero sweeps.
take_stage0_lock() {
  [ "${STAGE1_LOCKED:-0}" = 1 ] && return 0
  exec 8> "$L/stage0.lock"
  note "waiting for $L/stage0.lock"
  flock 8
  export STAGE1_LOCKED=1
}

# adopt_loop_keep: a jetc0-loop.sh keep dir ($L/loop) holds jetc0 and the
# retained stage-zero source (stage-zero.rs/.image/.ffi/.stamp, compiler-project)
# but no jetc0.env or artifact-main.rs: its frontend stops after emit, before
# retain_stage_zero. Derive both from what the loop retained, whenever jetc0 is
# newer than jetc0.env (each loop iteration): repo = the tree of the emitted
# Runner.rs #[path], package = the backend manifest's name, id = its
# .bootstrap-artifact-id, task_roots = the stamp's; artifact-main.rs = the repo's
# GENERATED_ARTIFACT_MAIN, which must be the tail of stage-zero.rs (the harness
# appended it there).
adopt_loop_keep() {
  [ -x "$KEEP/jetc0" ] && [ -f "$KEEP/stage-zero.rs" ] && [ -f "$KEEP/stage-zero.stamp" ] || return 0
  [ -f "$KEEP/jetc0.env" ] && [ "$KEEP/jetc0.env" -nt "$KEEP/jetc0" ] && return 0
  local repo task_roots package id size
  repo=$(grep -o -m1 '#\[path = "[^"]*/Compiler/Bootstrap/Runner.rs"\]' "$KEEP/stage-zero.rs" | sed 's|^#\[path = "||; s|/Compiler/Bootstrap/Runner.rs"\]$||')
  [ -n "$repo" ] || die "cannot adopt $KEEP: no Runner.rs #[path] in stage-zero.rs"
  task_roots=$(sed -n 's/^task_roots=//p' "$KEEP/stage-zero.stamp")
  package=$(sed -n 's/^name = "\(.*\)"$/\1/p' "$KEEP/stage-zero/Cargo.toml" 2> /dev/null | head -n1)
  id=$(cat "$KEEP/stage-zero/.bootstrap-artifact-id" 2> /dev/null)
  extract_artifact_main "$repo" "${task_roots:-false}" "$KEEP/artifact-main.rs.tmp" || die "cannot extract the artifact main from $repo"
  size=$(stat -c %s "$KEEP/artifact-main.rs.tmp")
  tail -c "$size" "$KEEP/stage-zero.rs" | cmp -s - "$KEEP/artifact-main.rs.tmp" ||
    die "cannot adopt $KEEP: $repo's GENERATED_ARTIFACT_MAIN is not the tail of stage-zero.rs (Tests.rs changed after the loop's test binary)"
  mv "$KEEP/artifact-main.rs.tmp" "$KEEP/artifact-main.rs"
  printf 'repo=%s\nsession=jetc0-loop\nid=%s\ntask_roots=%s\npackage=%s\n' "$repo" "${id:-?}" "${task_roots:-false}" "${package:-jetc0_loop}" > "$KEEP/jetc0.env"
  note "adopted jetc0-loop keep $KEEP (repo $repo, package ${package:-?})"
}

# Reads $KEEP/jetc0.env (written by retain_stage_zero in Tests.rs) into
# KEEP_REPO, KEEP_TASK_ROOTS, KEEP_ID, KEEP_PACKAGE, and pins jetc0's build
# inputs into KEEP_PIN (pin_inputs).
load_keep() {
  adopt_loop_keep
  [ -x "$KEEP/jetc0" ] || die "no retained jetc0 at $KEEP/jetc0 (run the stage-zero test with JET_STAGE_ZERO_KEEP=$KEEP, or retain-jetc0.sh)"
  [ -f "$KEEP/jetc0.env" ] || die "no $KEEP/jetc0.env beside jetc0"
  local key value
  while IFS='=' read -r key value; do
    case $key in
      repo) KEEP_REPO=$value ;;
      task_roots) KEEP_TASK_ROOTS=$value ;;
      id) KEEP_ID=$value ;;
      package) KEEP_PACKAGE=$value ;;
    esac
  done < "$KEEP/jetc0.env"
  [ -n "${KEEP_REPO:-}" ] && [ -d "$KEEP_REPO" ] || die "jetc0.env repo '${KEEP_REPO:-}' is missing; jetc0 reads Compiler/Bootstrap/Host/*.rs from it at run time"
  # Runner mode packages these by absolute #[path] (Runner.rs package_bootstrap_artifact);
  # a missing one fails deep inside the run as an opaque codec error.
  local host
  for host in Runner.rs Host/Native.rs Host/NativeAdapter.rs Host/RuntimeMirCodec.rs Host/DiagnosticCodec.rs Host/RuntimeMir.rs Host/CompilerImage.rs Host/EntryCodec.rs; do
    [ -f "$KEEP_REPO/Compiler/Bootstrap/$host" ] || die "$KEEP_REPO/Compiler/Bootstrap/$host is missing; generated compilers package it at run time"
  done
  [ -f "$KEEP/artifact-main.rs" ] || die "missing $KEEP/artifact-main.rs"
  if [ "${STAGE1_BACKEND:-cargo}" = cargo ]; then
    KEEP_HOST=${STAGE1_HOST_TEST:-$(sed -n 's/^host=\(.*\)@[0-9]*@[0-9]*$/\1/p' "$KEEP/stage-zero.stamp" 2> /dev/null)}
    [ -x "${KEEP_HOST:-}" ] || die "no host test binary '${KEEP_HOST:-}' (stage-zero.stamp host=, or STAGE1_HOST_TEST); it writes backend projects"
    [ -x "$KEEP_REPO/Tools/agent/jet-env" ] || die "$KEEP_REPO/Tools/agent/jet-env is missing; backend builds run cargo through it"
    [ -f "$KEEP_REPO/Cargo.lock" ] || die "$KEEP_REPO/Cargo.lock is missing; backend builds start from the repository lockfile"
  fi
  [ -x "$(stage1_perf)" ] || note "no perf binary (STAGE1_PERF); timeouts will not leave a profile"
  pin_inputs
}

# pin_inputs: a frozen copy of the retained repo (every tracked and untracked,
# non-ignored file) in $KEEP/inputs, made once per retained jetc0. jetc0.env's
# repo is a live worktree (rel-overlay2) that overlay-sync.sh rewrites; every
# later stage compiles Host/Runner (#[path]), the path-dependency crates and
# their compiled-in Core texts from the pin instead (build_backend), and
# fixedpoint.sh assembles jetc0's own compiler source from it, so a sync after
# stage zero cannot change what jetc1 and jetc2 are built from. Run it right
# after stage zero (pin-inputs.sh); pinning later warns when the repo changed
# after jetc0 was retained.
pin_inputs() {
  local pin=$KEEP/inputs stamp late
  stamp="jetc0_id=${KEEP_ID:-?} jetc0_mtime=$(stat -c %Y "$KEEP/jetc0") repo=$KEEP_REPO"
  KEEP_PIN=$pin
  [ -f "$pin/.pinned" ] && [ "$(sed -n 1p "$pin/.pinned")" = "$stamp" ] && return 0
  # Pinning writes the shared keep dir: serialize with other stage-one runs.
  take_stage0_lock
  [ -f "$pin/.pinned" ] && [ "$(sed -n 1p "$pin/.pinned")" = "$stamp" ] && return 0
  late=$(repo_changed_since "$KEEP_REPO" "$KEEP/jetc0")
  [ "$late" = 0 ] || note "WARNING: pinning $KEEP_REPO late: $late build inputs changed after jetc0 was retained, so the pin may differ from jetc0's own inputs (run stage1/pin-inputs.sh right after stage zero)"
  rm -rf "$pin.tmp" && mkdir -p "$pin.tmp" || die "cannot create $pin.tmp"
  ( cd "$KEEP_REPO" && git ls-files -z -co --exclude-standard | tar --null --ignore-failed-read -T - -cf - 2> /dev/null ) | tar -xf - -C "$pin.tmp" || die "cannot pin $KEEP_REPO into $pin.tmp"
  # The Compiler/ unit the pin assembles must be the one jetc0 compiled
  # (retain_stage_zero_source keeps it in $KEEP/compiler-project).
  local unit=absent
  if [ -f "$KEEP/compiler-project/src/compiler.jet" ]; then
    assemble_compiler "$pin.tmp" "$SCRATCH/stage1-pin-unit.$$" "${KEEP_TASK_ROOTS:-false}" &&
      cmp -s "$SCRATCH/stage1-pin-unit.$$/project/src/compiler.jet" "$KEEP/compiler-project/src/compiler.jet" &&
      cmp -s "$SCRATCH/stage1-pin-unit.$$/project/package.jet" "$KEEP/compiler-project/package.jet" && unit=same || unit=differs
    rm -rf "$SCRATCH/stage1-pin-unit.$$"
    [ "$unit" = same ] || note "WARNING: the pin's assembled Compiler/ unit differs from jetc0's own ($KEEP/compiler-project): jetc0 implements different source than the pin"
  fi
  printf '%s\nfingerprint=%s\npinned_at=%s late_changes=%s\ncompiler_unit=%s\n' "$stamp" "$(pin_fingerprint "$pin.tmp")" "$(date -Is)" "$late" "$unit" > "$pin.tmp/.pinned"
  rm -rf "$pin" && mv "$pin.tmp" "$pin" || die "cannot publish $pin"
  note "pinned $KEEP_REPO -> $pin ($(sed -n 2p "$pin/.pinned"), compiler_unit=$unit)"
}

# pin_fingerprint DIR: digest of the build inputs under DIR (manifests, crates,
# Source, Core, Compiler/Bootstrap).
pin_fingerprint() {
  ( cd "$1" && find Cargo.toml Cargo.lock crates Source Core Compiler/Bootstrap -type f -print0 2> /dev/null \
      | sort -z | xargs -0 sha256sum | sha256sum | cut -c1-16 )
}

# extract_artifact_main REPO TASK_ROOTS OUT: GENERATED_ARTIFACT_MAIN from the
# repo's Compiler/Bootstrap/Tests.rs, with the task-root regions dropped unless
# TASK_ROOTS=true (append_generated_artifact_main). retain_stage_zero writes this
# file itself; this is for stage-zero runs that predate it.
extract_artifact_main() {
  node -e '
    const [tests, taskRoots, out] = process.argv.slice(1);
    const text = require("fs").readFileSync(tests, "utf8");
    const open = "const GENERATED_ARTIFACT_MAIN: &str = r#\"";
    const start = text.indexOf(open);
    if (start < 0) { console.error("no GENERATED_ARTIFACT_MAIN in " + tests); process.exit(1); }
    const body = text.slice(start + open.length, text.indexOf("\"#;", start));
    const BEGIN = "// bootstrap:task-roots-begin", END = "// bootstrap:task-roots-end";
    let result = "";
    if (taskRoots === "true") result = body;
    else {
      let rest = body;
      for (let begin; (begin = rest.indexOf(BEGIN)) >= 0;) {
        result += rest.slice(0, begin);
        const end = rest.indexOf(END, begin);
        if (end < 0) { console.error("unterminated task-root region"); process.exit(1); }
        rest = rest.slice(end + END.length);
      }
      result += rest;
    }
    require("fs").writeFileSync(out, result);' "$1/Compiler/Bootstrap/Tests.rs" "$2" "$3"
}

stage1_perf() {
  local perf=${STAGE1_PERF:-}
  [ -n "$perf" ] || perf=$(command -v perf 2> /dev/null) || perf=/nix/store/v5fs9a5z17j8pfd2kfxs44hv5p3yq6ap-perf-linux-7.0.11/bin/perf
  echo "$perf"
}

# descendant_with_exe ROOT_PID EXE: the first descendant of ROOT_PID running EXE.
descendant_with_exe() {
  local child
  for child in $(pgrep -P "$1"); do
    [ "$(readlink "/proc/$child/exe" 2> /dev/null)" = "$2" ] && { echo "$child"; return 0; }
    descendant_with_exe "$child" "$2" && return 0
  done
  return 1
}

# perf_sample PID SECONDS OUT: flat user-space profile (top symbols name the
# phase: parser, sema, lowering, emission) appended to OUT.
perf_sample() {
  local perf data
  perf=$(stage1_perf)
  [ -x "$perf" ] || { echo "(no perf)" >> "$3"; return 0; }
  data=$(mktemp "$SCRATCH/stage1-perf.XXXXXX")
  "$perf" record -q -F 99 -e cpu-clock:u -p "$1" -o "$data" -- sleep "$2" > /dev/null 2>&1
  "$perf" report -q -i "$data" --no-children --sort dso,sym --stdio 2> /dev/null | sed -n 1,40p | cut -c1-200 >> "$3"
  rm -f "$data"
}

# perf_callgraph PID SECONDS OUT: call-graph profile (perf record -g) of PID;
# the caller-ordered report goes to OUT, the raw data beside it (OUT.data) for
# `perf report -i OUT.data --children`. Frame-pointer stacks need a binary built
# with -C force-frame-pointers=yes (JET_STAGE_ZERO_PROFILE=release does);
# STAGE1_CALLGRAPH=dwarf unwinds with DWARF instead (any binary with unwind
# tables, e.g. a dev or loop jetc0).
perf_callgraph() {
  local perf mode=(-F 199 -g)
  perf=$(stage1_perf)
  [ -x "$perf" ] || { echo "(no perf)" >> "$3"; return 0; }
  [ "${STAGE1_CALLGRAPH:-fp}" = dwarf ] && mode=(-F 49 --call-graph dwarf,16384)
  "$perf" record -q "${mode[@]}" -e cpu-clock:u -p "$1" -o "$3.data" -- sleep "$2" > /dev/null 2>&1
  "$perf" report -q -i "$3.data" --children --sort sym -g caller,0.5,callee,function,percent --stdio 2> /dev/null | sed -n 1,400p | cut -c1-220 >> "$3"
}

# jetc_watch WRAPPER_PID BINARY LIMIT LOG: while the run lives, every
# STAGE1_SAMPLE_SECS append RSS/peak RSS and a 10 s profile to LOG.progress
# (STAGE1_SAMPLE_CALLGRAPH=1: also a 10 s call-graph profile to LOG.cg+<secs>.txt);
# after STAGE1_STALL_SECS (default 900: the 15-minute stall rule) write one
# 60 s call-graph profile to LOG.stall.txt (+ .data) so a slow run is
# diagnosed while it runs; 75 s before the timeout, write a 30 s profile to
# LOG.perf.txt so a timeout leaves evidence of where the time went.
jetc_watch() {
  local wrapper=$1 exe limit=$3 log=$4 pid="" started now next every stall
  exe=$(realpath "$2")
  started=$(date +%s)
  every=${STAGE1_SAMPLE_SECS:-600}
  next=$(( started + every ))
  while kill -0 "$wrapper" 2> /dev/null; do
    sleep 5
    now=$(date +%s)
    if [ -z "$pid" ] || ! kill -0 "$pid" 2> /dev/null; then
      pid=$(descendant_with_exe "$wrapper" "$exe") || { pid=""; continue; }
    fi
    if [ "$every" -gt 0 ] && [ "$now" -ge "$next" ]; then
      echo "== +$(( now - started ))s pid $pid $(grep -E '^(VmRSS|VmHWM)' "/proc/$pid/status" 2> /dev/null | tr -s ' \t\n' ' ')" >> "$log.progress"
      perf_sample "$pid" 10 "$log.progress"
      if [ "${STAGE1_SAMPLE_CALLGRAPH:-0}" = 1 ]; then
        echo "== +$(( now - started ))s pid $pid $(grep -E '^(VmRSS|VmHWM)' "/proc/$pid/status" 2> /dev/null | tr -s ' \t\n' ' ')" > "$log.cg+$(( now - started ))s.txt"
        perf_callgraph "$pid" 10 "$log.cg+$(( now - started ))s.txt"
      fi
      next=$(( now + every ))
    fi
    stall=${STAGE1_STALL_SECS:-900}
    if [ "$stall" -gt 0 ] && [ "$now" -ge $(( started + stall )) ] && [ ! -e "$log.stall.txt" ]; then
      echo "== +$(( now - started ))s: stall rule (${stall}s): 60 s call-graph profile of pid $pid; $(grep -E '^(VmRSS|VmHWM)' "/proc/$pid/status" 2> /dev/null | tr -s ' \t\n' ' ')" > "$log.stall.txt"
      perf_callgraph "$pid" 60 "$log.stall.txt"
    fi
    if [ "$now" -ge $(( started + limit - 75 )) ] && [ ! -e "$log.perf.txt" ]; then
      echo "== +$(( now - started ))s of ${limit}s: 30 s profile of pid $pid before the timeout; $(grep -E '^(VmRSS|VmHWM)' "/proc/$pid/status" 2> /dev/null | tr -s ' \t\n' ' ')" > "$log.perf.txt"
      perf_sample "$pid" 30 "$log.perf.txt"
    fi
  done
}

# jetc_verdict RC LOG: one line naming how a compiler run ended, also written
# to LOG.verdict.
jetc_verdict() {
  local rc=$1 log=$2 verdict
  case $rc in
    0) verdict="exited 0" ;;
    124) verdict="TIMEOUT after ${STAGE1_JETC_TIMEOUT:-14400}s; profile: $log.perf.txt, samples: $log.progress" ;;
    137) verdict="KILLED (SIGKILL: memory cap ${STAGE1_JETC_MEM:-20G} or OOM); samples: $log.progress" ;;
    *) verdict="exited $rc" ;;
  esac
  grep -q 'has overflowed its stack' "$log" 2> /dev/null && verdict="STACK OVERFLOW (native thread stack): $(grep -m1 'has overflowed its stack' "$log")"
  grep -q 'E3012' "$log" 2> /dev/null && verdict="JET FRAME LIMIT E3012 (JET_RUNTIME_STACK_LIMIT=1024 Jet frames): $(grep -m1 'E3012' "$log" | cut -c1-200)"
  echo "$verdict" > "$log.verdict"
  echo "$verdict"
}

# jetc BINARY MODE SOURCE_ROOT ENTRY OUTPUT RECEIPT LOG
# One compiler run under a memory cap, a timeout and an explicit environment
# (see the header). Returns the process status; LOG.verdict says how it ended.
jetc() {
  local bin=$1 mode=$2 root=$3 entry=$4 out=$5 receipt=$6 log=$7
  local store=${log%.log}.store limit=${STAGE1_JETC_TIMEOUT:-14400} wrapper watcher rc name standin=() batch=()
  rm -rf "$out" "${out%.rs}.image" "$receipt" "$store" "$log.progress" "$log.perf.txt" "$log.stall.txt" "$log.stall.txt.data" "$log.verdict" "$log".cg+*
  mkdir -p "$store"
  # The dry-run stand-in (standin/standin-jetc.mjs) is configured by STANDIN_*.
  if [ "${STAGE1_BACKEND:-cargo}" = standin ]; then
    for name in STANDIN_JET STANDIN_REPO STANDIN_NONDETERMINISTIC STANDIN_PANIC_ON STANDIN_CRASH_ON STANDIN_HANG_ON; do
      [ -n "${!name:-}" ] && standin+=("$name=${!name}")
    done
  fi
  [ -n "${STAGE1_JETC_BATCH:-}" ] && batch=(JET_BOOTSTRAP_BATCH="$STAGE1_JETC_BATCH")
  # The compiler's streaming trace (crates/jet-driver/src/Trace.rs) passes through when requested.
  [ -n "${JET_TRACE_FILE:-}" ] && batch+=(JET_TRACE_FILE="$JET_TRACE_FILE")
  [ -n "${JET_TRACE_DETAIL:-}" ] && batch+=(JET_TRACE_DETAIL="$JET_TRACE_DETAIL")
  ( ulimit -c 0
    ulimit -s "${STAGE1_MAIN_STACK_KIB:-1048576}"
    exec systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax="${STAGE1_JETC_MEM:-20G}" -p MemorySwapMax=0 \
      env -i HOME="$HOME" PATH="$PATH" TMPDIR="$SCRATCH" LC_ALL=C \
          JET_BOOTSTRAP_MODE="$mode" JET_BOOTSTRAP_FACTORY_TIER="${STAGE1_TIER:-aot}" \
          JET_BOOTSTRAP_SOURCE_ROOT="$root" JET_BOOTSTRAP_ENTRY="$entry" \
          JET_BOOTSTRAP_OUTPUT="$out" JET_BOOTSTRAP_RECEIPT="$receipt" \
          JET_STORE_DIR="$store" RUST_MIN_STACK=8388608 RUST_BACKTRACE=1 "${standin[@]}" "${batch[@]}" \
      timeout -k 60 "$limit" "$bin" ) > "$log" 2>&1 &
  wrapper=$!
  jetc_watch "$wrapper" "$bin" "$limit" "$log" &
  watcher=$!
  { wait "$wrapper"; } 2> /dev/null
  rc=$?
  kill "$watcher" 2> /dev/null
  wait "$watcher" 2> /dev/null
  jetc_verdict "$rc" "$log" > /dev/null
  return "$rc"
}

# receipt_ok RECEIPT MODE: the assert_runner_receipt / assert_factory_receipt checks.
receipt_ok() {
  local receipt=$1 mode=$2
  [ -f "$receipt" ] || return 1
  grep -qx "mode=$mode" "$receipt" && grep -qx "complete=true" "$receipt" &&
    grep -qE '^source_bytes=[1-9]' "$receipt"
}

# receipt_reports RECEIPT [MAP]: CODE<TAB>location<TAB>message per report.
receipt_reports() {
  node "$STAGE1_TOOLS/reports.mjs" "$@"
}

# assemble_compiler SOURCE_REPO DIR TASK_ROOTS: the Compiler/Bootstrap/assemble.mjs
# unit in DIR/project (package.jet + src/compiler.jet) and its source map in
# DIR/compiler.map.json. The assembler writes under $HOME/.cache/jet-dev/
# compiler-bootstrap, which the stage-zero test and unitcheck share, so it runs
# with a private HOME. TASK_ROOTS=true passes --task-roots, which adds the
# task-root fixture the way assemble_compiler_sources does for the
# self-compile harness.
assemble_compiler() {
  local repo=$1 dir=$2 task_roots=$3
  local home=$dir/assemble-home flags=()
  [ "$task_roots" = true ] && flags=(--task-roots)
  rm -rf "$home" "$dir/project" "$dir/compiler.map.json"
  mkdir -p "$home"
  ( cd "$repo" && env -u JET_BOOTSTRAP_SOURCE_ROOT HOME="$home" node Compiler/Bootstrap/assemble.mjs "${flags[@]}" ) > "$dir/assemble.log" 2>&1 || return 1
  mv "$home/.cache/jet-luna/compiler-bootstrap/project" "$dir/project" &&
    mv "$home/.cache/jet-luna/compiler-bootstrap/compiler.map.json" "$dir/compiler.map.json" &&
    rm -rf "$home" || return 1
}

# build_backend PROJECT_DIR PACKAGE SOURCE_FILE LOG -> prints the binary path.
# The backend project is the one build_backend_artifact (Tests.rs) builds,
# written by the same code: the host test binary's ignored
# bootstrap_backend_project_from_env test (write_backend_project): the
# unit-split workspace (units under ~/.cache/jet-dev/compiler-bootstrap/units/
# PACKAGE), src/compiler.image from <source>.image, the FFI bridge from
# <source>.ffi (only the retained stage-zero source names one; generated
# compilers emit with none, as the harness builds stage one and two), the
# manifest, identity build script and Cargo.lock. Every input path into the
# retained repo points at the pin (pin_inputs): the hook's repo is the pin
# (manifest path dependencies, build-script identity root, lockfile), and the
# emitted source's #[path] Host/Runner modules are rewritten to it first. The
# emitted BOOTSTRAP_CANONICAL_SOURCE_ROOT text stays as jetc0 baked it, so every
# stage emits the same text. `cargo build` (debug, no incremental, no sccache)
# runs through the retained repo's jet-env, whose checkout must contain
# CARGO_TARGET_DIR: <repo>/target-stage1 (overlay-sync.sh keeps target-* dirs).
build_backend() {
  local project=$1 package=$2 source=$3 log=$4
  mkdir -p "$project/src"
  if [ "${STAGE1_BACKEND:-cargo}" = standin ]; then
    # Dry-run backend: the stand-in compiler's "source" is already a script.
    cp "$source" "$project/$package" && chmod +x "$project/$package"
    echo "standin backend: $source -> $project/$package" > "$log"
    echo "$project/$package"
    return 0
  fi
  # --cap-lints=warn: the harness's backend builds inherit it from the stage-zero
  # test's RUSTFLAGS (stage0-rel.sh, jetc0-loop.sh); the repository crates'
  # #![deny(warnings)] would otherwise stop the build on any new warning.
  local repo=$KEEP_REPO pin=$KEEP_PIN target=$KEEP_REPO/target-stage1 pinned ext profile_dir=debug profile_args=() rustflags=--cap-lints=warn
  local hook_profile=${STAGE1_CARGO_PROFILE:-dev}
  # Same profile as the stage-zero test's build_backend_artifact: the hook writes
  # the jet-stage-release profile (with opt-level 0 rows for cold shards) into the
  # manifest when JET_STAGE_ZERO_PROFILE=release.
  [ "$hook_profile" = release ] && { profile_dir=jet-stage-release; profile_args=(--profile jet-stage-release); rustflags="$rustflags -C force-frame-pointers=yes"; }
  sed "s|#\[path = \"$repo/|#[path = \"$pin/|" "$source" > "$project/pinned.rs"
  grep -q "#\[path = \"$repo/" "$project/pinned.rs" && die "unpinned #[path] left in $project/pinned.rs"
  # Only a compiler's output splices in the Host/Runner modules; an ordinary
  # program (goldens.sh cases) has none.
  pinned=$(grep -c "#\[path = \"$pin/" "$project/pinned.rs")
  if [ "$pinned" -gt 0 ] || grep -q 'jet_bootstrap_compile' "$project/pinned.rs"; then
    [ "$pinned" -ge 7 ] || die "expected the 7 Host/Runner #[path] modules (Runner.rs package_bootstrap_artifact) pinned in $project/pinned.rs, found $pinned"
  fi
  # Runner writes a compiler's image beside its output (<output>.image;
  # with_artifact_main carries it to <source>.image); the retained stage-zero
  # source has its bridge record beside it (stage-zero.ffi).
  for ext in image ffi; do
    if [ -f "${source%.rs}.$ext" ]; then cp "${source%.rs}.$ext" "$project/pinned.$ext"; else rm -f "$project/pinned.$ext"; fi
  done
  # Generated compilers emit the extern declarations of the inline-asm/C FFI
  # functions (jet_inline_jet_ffi_*) but no bridge crate that defines them
  # (only stage zero's Rust frontend prepares one: stage-zero.ffi), so their
  # link fails. STAGE1_REUSE_FFI=1 links the stage-zero bridge instead, to get
  # past that known gap and see the next failures; the harness has no such step.
  if [ "${STAGE1_REUSE_FFI:-0}" = 1 ] && [ ! -f "$project/pinned.ffi" ] && [ -s "$KEEP/stage-zero.ffi" ] &&
     grep -q 'jet_inline_jet_ffi_' "$project/pinned.rs"; then
    cp "$KEEP/stage-zero.ffi" "$project/pinned.ffi"
    note "STAGE1_REUSE_FFI: linking $(basename "$project") with the stage-zero FFI bridge ($(sed -n 1p "$KEEP/stage-zero.ffi"))"
  fi
  rm -f "$project/src/main.rs" "$project/src/compiler.image" "$project/Cargo.toml"
  systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax="${STAGE1_CARGO_MEM:-20G}" -p MemorySwapMax=0 \
    env JET_STAGE_ZERO_PROFILE="$hook_profile" JET_BACKEND_PROJECT_REPO="$pin" JET_BACKEND_PROJECT_DIR="$project" \
        JET_BACKEND_PROJECT_PACKAGE="$package" JET_BACKEND_PROJECT_SOURCE="$project/pinned.rs" \
    "$KEEP_HOST" --ignored --exact bootstrap_tests::bootstrap_backend_project_from_env < /dev/null > "$log" 2>&1 || return 1
  grep -q '^test result: ok\. 1 passed' "$log" && [ -f "$project/src/main.rs" ] && [ -f "$project/Cargo.toml" ] ||
    die "backend project hook did not write $project (host $KEEP_HOST predates bootstrap_backend_project_from_env?): $log"
  if grep -q 'include_bytes!("compiler.image")' "$project/src/main.rs"; then
    [ -f "$project/src/compiler.image" ] || die "missing compiler image ${source%.rs}.image for $source"
  fi
  local lane=(systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax="${STAGE1_CARGO_MEM:-20G}" -p MemorySwapMax=0)
  [ -n "${STAGE1_CARGO_LANE:-}" ] && lane=("$JET_AGENT_TOOLS/cache/laneB.sh" "$STAGE1_CARGO_LANE")
  ( cd "$repo" && flock "$L/cargo-stage1-$(basename "$repo").lock" \
      "${lane[@]}" \
      env -u RUSTC_WRAPPER CARGO_TARGET_DIR="$target" CARGO_INCREMENTAL=0 JET_NO_SCCACHE=1 \
          CARGO_BUILD_JOBS="${STAGE1_CARGO_JOBS:-4}" TMPDIR="$SCRATCH" RUST_MIN_STACK=268435456 \
          RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$rustflags" \
      timeout "${STAGE1_CARGO_TIMEOUT:-7200}" "$repo/Tools/agent/jet-env" \
      cargo build "${profile_args[@]}" --manifest-path "$project/Cargo.toml" --bin "$package" < /dev/null ) >> "$log" 2>&1 || return 1
  [ -x "$target/$profile_dir/$package" ] || return 1
  echo "$target/$profile_dir/$package"
}

# with_artifact_main RAW OUT: the stage-one source the harness builds is the
# Runner's raw output plus the generated-artifact main (append_generated_artifact_main).
with_artifact_main() {
  cat "$1" "$KEEP/artifact-main.rs" > "$2"
  # Runner mode writes the compiler image beside its output (<raw>.image).
  if [ -f "${1%.rs}.image" ]; then cp "${1%.rs}.image" "${2%.rs}.image"; else rm -f "${2%.rs}.image"; fi
}

# pin_unchanged: the pin still matches the fingerprint recorded when it was made.
pin_unchanged() {
  [ "$(pin_fingerprint "$KEEP_PIN")" = "$(sed -n 's/^fingerprint=//p' "$KEEP_PIN/.pinned")" ]
}

# repo_changed_since REPO FILE: count of fingerprinted inputs modified after FILE.
repo_changed_since() {
  local since
  since=$(stat -c %Y "$2")
  ( cd "$1" && git ls-files -z -co --exclude-standard -- Cargo.toml Cargo.lock crates Source Core Compiler/Bootstrap \
      | xargs -0 -r stat -c %Y 2> /dev/null | awk -v since="$since" '$1 > since' | wc -l )
}
