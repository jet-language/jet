#!/usr/bin/env bash
JET_AGENT_TOOLS="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JET_REPO="${JET_REPO:-$(cd "$JET_AGENT_TOOLS/../.." && pwd)}"
STAGE1_TOOLS="$JET_AGENT_TOOLS/stage1"
# fast-loop.sh: jetc0-loop.sh ($L/jetc0-loop.sh) with a fast-candidate backend
# profile, for bug discovery. Same phases, outputs and keep-dir layout; only the
# Cargo profile of the generated unit crates differs:
#   jetc_* hot crates   opt-level $FAST_OPT (default 1; loop: 2), debug = false
#                       (loop: line-tables-only), incremental = false (the
#                       emitted source of a package and of every package after
#                       it in sources.list shifts on any edit, so incremental
#                       state is never reusable and only costs time),
#                       debug-assertions = false, overflow checks on (as loop).
#   jetc_*_cold*        opt-level 0, debug = false, incremental = false.
#   jet_runtime         exactly the loop's settings (artifact shared with it).
#   jetc0_loop (bin)    strip = "debuginfo": the link skips the deps' DWARF.
#   everything else     unchanged dev profile, so the repository crates
#                       (jet, jet-codegen, jet-jit, cranelift, ...) are the
#                       loop's cached artifacts in the same target dir.
# Cargo hashes the profile into each unit's file names, so fast and loop
# artifacts of the same crate coexist in rel-overlay3/target-loop: switching
# between fast-cand and chain-cand rebuilds only what changed.
# CARGO_INCREMENTAL is removed from the cargo environment (jet-env exports it
# for JET_CARGO_INCREMENTAL=1, and the variable overrides every package's
# profile); the dev default (incremental on) still covers the path crates.
#
# usage: fast-loop.sh   (env as jetc0-loop.sh: JETC0_LOOP_DIR, _OVERLAY, _HOST,
#        _JOBS, _LIVE, _UNTIL; plus FAST_OPT, JETC0_LOOP_MEM (frontend/project
#        cap, default 22G), FAST_BACKEND_MEM (cargo cap, default 12G),
#        FAST_INCREMENTAL=1 (keep incremental on the jetc crates))
set -u
L=$HOME/.cache/jet-dev
R=$JET_REPO
LIVE=${JETC0_LOOP_LIVE:-$R}
UNTIL=${JETC0_LOOP_UNTIL:-}
O=${JETC0_LOOP_OVERLAY:-$R/.agent-worktrees/rel-overlay3}
D=${JETC0_LOOP_DIR:?JETC0_LOOP_DIR (keep dir)}
PKG=jetc0_loop
HELLO_PKG=jetc0_loop_hello
MEM=${JETC0_LOOP_MEM:-22G}
BMEM=${FAST_BACKEND_MEM:-12G}
JOBS=${JETC0_LOOP_JOBS:-4}
FAST_OPT=${FAST_OPT:-1}
FAST_INCREMENTAL=${FAST_INCREMENTAL:-0}
export JETC0_KEEP=$D
. "$STAGE1_TOOLS/lib.sh"
mkdir -p "$D"

phases=()
t0=$SECONDS
mark=$SECONDS
phase() {
  phases+=("$1	$((SECONDS - mark))	${2:-}")
  mark=$SECONDS
  if [ "$1" = "$UNTIL" ]; then report "stopped after $1 (JETC0_LOOP_UNTIL)"; exit 0; fi
}
report() {
  local row total=$((SECONDS - t0)) stamp
  stamp=$(date +%FT%T)
  echo "== jetc0-loop $stamp: ${1}"
  for row in "${phases[@]}"; do
    printf '%-9s %5ss  %s\n' $(cut -f1 <<< "$row") "$(cut -f2 <<< "$row")" "$(cut -f3- <<< "$row")"
    echo "$stamp	$row" >> "$D/phases.tsv"
  done
  printf '%-9s %5ss  %s\n' total "$total" "$1"
  echo "$stamp	total	$total	$1" >> "$D/phases.tsv"
}
fail() { phases+=("$1	$((SECONDS - mark))	FAILED"); report "FAILED in $1: $2"; exit 1; }
# One capped step in the stage-zero lane (the stage1/lib.sh pattern).
capped() {
  systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax="$MEM" -p MemorySwapMax=0 \
    bash -c 'ulimit -c 0; exec "$@"' capped "$@"
}
capped_backend() {
  systemd-run --user --slice=jetwork.slice --scope -q -p MemoryMax="$BMEM" -p MemorySwapMax=0 \
    bash -c 'ulimit -c 0; exec "$@"' capped "$@"
}

HOST=${JETC0_LOOP_HOST:-$(sed -n 's/^host=\(.*\)@[0-9]*@[0-9]*$/\1/p' "$L/stage0/stage-zero.stamp" 2> /dev/null)}
[ -x "${HOST:-}" ] || die "no stage-zero test binary '${HOST:-}' (host= of $L/stage0/stage-zero.stamp, or JETC0_LOOP_HOST)"

take_stage0_lock
mark=$SECONDS
t0=$SECONDS

# --- sync -------------------------------------------------------------------
jet_only=(--include='*/' --include='*.jet' --include='sources.list' --exclude='*')
if [ $# -gt 0 ]; then
  base=$L/unitcheck-gate/good-src
  [ "$L/unitcheck/good-src" -nt "$base" ] && base=$L/unitcheck/good-src
  [ -d "$base/Compiler" ] || die "no clean-gate base $base/Compiler"
  mkdir -p "$D/src/Compiler"
  rsync -rc --delete "${jet_only[@]}" "$base/Compiler/" "$D/src/Compiler/" || die "cannot stage $base"
  for p in "$@"; do
    case $p in Compiler/*) ;; *) die "$p: only Compiler/ files ride this loop" ;; esac
    if [ -e "$LIVE/$p" ]; then mkdir -p "$D/src/$(dirname "$p")" && cp "$LIVE/$p" "$D/src/$p"; else rm -f "$D/src/$p"; fi
  done
  # Manifest entries the base predates come from the live tree (as isocheck.sh).
  while read -r p; do
    case $p in '' | '#'*) continue ;; esac
    [ -e "$D/src/$p" ] || { [ -e "$LIVE/$p" ] && mkdir -p "$D/src/$(dirname "$p")" && cp "$LIVE/$p" "$D/src/$p"; }
  done < "$D/src/Compiler/Bootstrap/sources.list"
  from=$D/src/Compiler
  what="clean gate $(basename "$(dirname "$base")") + $*"
else
  from=$LIVE/Compiler
  what="$LIVE/Compiler"
fi
changed=$(rsync -rc --delete --itemize-changes "${jet_only[@]}" "$from/" "$O/Compiler/" | grep -c '^[<>*]') || true
# Drift the loop does not carry: Rust or Core newer than the test binary.
drift=$( { (cd "$O" && find crates Source Compiler/Bootstrap Core Cargo.toml Cargo.lock build.rs -newer "$HOST" \
             \( -name '*.rs' -o -name '*.toml' -o -name Cargo.lock -o -path 'Core/*.jet' \) -not -path '*/target*' 2> /dev/null)
           (cd "$R" && find crates Source Compiler/Bootstrap Core -newer "$HOST" \( -name '*.rs' -o -path 'Core/*.jet' \) 2> /dev/null | sed 's|^|live:|'); } | sort -u)
[ -n "$drift" ] && note "WARNING: $(wc -l <<< "$drift") Rust/Core files changed after the test binary (not in this jetc0; stage zero carries them): $(head -5 <<< "$drift" | tr '\n' ' ')"
phase sync "$changed files from $what"

# --- frontend ---------------------------------------------------------------
started=$(date +%s)
# Through jet-env as under `cargo test`: the frontend reads the host target
# from the toolchain's `rustc -vV`.
( cd "$O" && capped "$O/Tools/agent/jet-env" env JET_BUILD_TARGET=x86_64-unknown-linux-gnu JET_STAGE_ZERO_KEEP="$D" JET_STAGE_ZERO_RESUME="$D" JET_STAGE_ZERO_STOP_AFTER_EMIT=1 \
    TMPDIR="$SCRATCH" RUST_BACKTRACE=0 "$HOST" --exact bootstrap_tests::bootstrap_stage_zero_hello --nocapture ) \
  < /dev/null > "$D/frontend.log" 2>&1
if ! grep -q 'stopped after emit' "$D/frontend.log"; then
  codes=$(grep -o 'code: "E[0-9]*"' "$D/frontend.log" | sort | uniq -c | sort -rn | head -8 | tr -s ' \n' ' ')
  fail frontend "$(grep -m1 -A2 'panicked at' "$D/frontend.log" | cut -c1-300 | tr '\n' ' ')${codes:+ codes:$codes} (log $D/frontend.log; mapped diagnostics: isocheck.sh)"
fi
if grep -q 'resuming from' "$D/frontend.log"; then
  phase frontend "skipped: sources unchanged"
else
  # reference.rs is written after emission, the stamp after packaging.
  phase frontend "check+lower+emit $(( $(stat -c %Y "$D/reference.rs") - started ))s, packaged at $(( $(stat -c %Y "$D/stage-zero.stamp") - started ))s"
fi

# --- project ----------------------------------------------------------------
units=$HOME/.cache/jet-dev/compiler-bootstrap/units/$PKG
stamp_units() { find "$units" -name lib.rs -printf '%T@ %h\n' 2> /dev/null | sort; }
units_before=$(stamp_units)
capped env JET_BACKEND_PROJECT_REPO="$O" JET_BACKEND_PROJECT_DIR="$D/stage-zero" JET_BACKEND_PROJECT_PACKAGE="$PKG" \
    JET_BACKEND_PROJECT_SOURCE="$D/stage-zero.rs" "$HOST" --ignored --exact bootstrap_tests::bootstrap_backend_project_from_env \
  < /dev/null > "$D/project.log" 2>&1
grep -q '^test result: ok\. 1 passed' "$D/project.log" || fail project "backend project hook failed: $D/project.log"
rewritten=$(comm -13 <(echo "$units_before") <(stamp_units) | awk '{print $2}' | xargs -r -n1 basename | tr '\n' ' ')
# Fast-candidate profile (header): jetc_* hot crates at $FAST_OPT, cold shards
# at 0, both without debuginfo or (unless FAST_INCREMENTAL=1) incremental
# state; jet_runtime keeps the loop's section verbatim; the binary is linked
# without the deps' debuginfo. Overflow checks keep the dev default.
python3 - "$D/stage-zero/Cargo.toml" "$FAST_OPT" "$FAST_INCREMENTAL" "$PKG" <<'PYEOF' || fail project "cannot set fast profile"
import re, sys
path, opt, incremental, pkg = sys.argv[1], sys.argv[2], sys.argv[3] == "1", sys.argv[4]
text = open(path).read()
def section(m):
    name = m.group(1)
    if name == "jet_runtime":
        return f'[profile.dev.package.{name}]\ndebug = "line-tables-only"\nopt-level = 2\ndebug-assertions = false\n'
    level = "0" if "_cold" in name else opt
    inc = "true" if incremental else "false"
    return f'[profile.dev.package.{name}]\ndebug = false\nopt-level = {level}\ndebug-assertions = false\nincremental = {inc}\n'
text, n = re.subn(r'(?m)^\[profile\.dev\.package\.((?:jetc_\w+)|jet_runtime)\]\n(?:[^\[\n].*\n)*', section, text)
if n == 0:
    sys.exit("no jetc_* profile sections")
text = text.rstrip("\n") + f'\n\n[profile.dev.package.{pkg}]\nstrip = "debuginfo"\n'
open(path, "w").write(text)
PYEOF
phase project "unit crates rewritten: ${rewritten:-none}"

# --- backend ----------------------------------------------------------------
# The loop's flags (--cap-lints=warn, frame pointers) and target dir, so the
# repository crates are shared with chain-cand builds; CARGO_INCREMENTAL is
# dropped after jet-env sets it, so the per-package `incremental` above holds.
T=$O/target-loop
backend() { # MANIFEST PACKAGE LOG
  ( cd "$O" && capped_backend env -u RUSTC_WRAPPER CARGO_TARGET_DIR="$T" JET_CARGO_INCREMENTAL=1 JET_NO_SCCACHE=1 \
      CARGO_BUILD_JOBS="$JOBS" TMPDIR="$SCRATCH" RUST_MIN_STACK=268435456 RUSTFLAGS="--cap-lints=warn -C force-frame-pointers=yes" \
      "$O/Tools/agent/jet-env" env -u CARGO_INCREMENTAL cargo build --manifest-path "$1" --bin "$2" --message-format short --timings ) \
    < /dev/null > "$3" 2>&1
}
backend "$D/stage-zero/Cargo.toml" "$PKG" "$D/backend.log" ||
  fail backend "$(grep -m5 -E '^error' "$D/backend.log" | cut -c1-200 | tr '\n' ' ') (log $D/backend.log)"
timing=$(ls -t "$T/cargo-timings/"cargo-timing-*.html 2> /dev/null | head -1)
[ -n "$timing" ] && node -e '
  const t = require("fs").readFileSync(process.argv[1], "utf8");
  const m = t.match(/const UNIT_DATA = (\[.*?\]);\n/s);
  if (!m) process.exit(0);
  for (const u of JSON.parse(m[1]).sort((a, b) => b.duration - a.duration))
    console.log(`${u.name}\t${u.duration.toFixed(1)}\t${u.mode}`);' "$timing" > "$D/backend-units.tsv"
built=$(awk -F'\t' '{printf "%s %.0fs, ", $1, $2}' "$D/backend-units.tsv" 2> /dev/null | cut -c1-400)
cp "$T/debug/$PKG" "$D/jetc0.partial" && mv "$D/jetc0.partial" "$D/jetc0" || fail backend "no $T/debug/$PKG"
phase backend "${built:-nothing rebuilt}"

# --- jetc0: runner over the hello fixture -----------------------------------
mkdir -p "$D/hello"
printf '%s' "$(sed -n '/^const SOURCE_FIXTURE_MANIFEST: &str = r#"/,/"#;/p' "$O/Compiler/Bootstrap/Tests.rs" \
  | sed '1s/^const SOURCE_FIXTURE_MANIFEST: &str = r#"//; $s/"#;$//')" > "$D/hello/package.jet"
printf 'fn run() {\n    print("hello")\n}\n' > "$D/hello/main.jet"
[ -s "$D/hello/package.jet" ] || fail jetc0 "cannot read SOURCE_FIXTURE_MANIFEST from Tests.rs"
STAGE1_JETC_TIMEOUT=${STAGE1_JETC_TIMEOUT:-3600} STAGE1_SAMPLE_SECS=${STAGE1_SAMPLE_SECS:-120} \
  jetc "$D/jetc0" runner "$D/hello" main.jet "$D/hello.raw.rs" "$D/hello.receipt" "$D/jetc0.log"
rc=$?
receipt_ok "$D/hello.receipt" runner ||
  fail jetc0 "$(cat "$D/jetc0.log.verdict" 2> /dev/null): $(grep -m1 -A1 'panicked at' "$D/jetc0.log" | cut -c1-300 | tr '\n' ' ') (log $D/jetc0.log, samples $D/jetc0.log.progress)"
phase jetc0 "rc=$rc receipt complete"

# --- hello: the emitted program's backend build and run ---------------------
mkdir -p "$D/hello-backend"
cp "$D/hello.raw.rs" "$D/hello-backend/source.rs"
rm -f "$D/hello-backend/source.image" "$D/hello-backend/source.ffi"
capped env JET_BACKEND_PROJECT_REPO="$O" JET_BACKEND_PROJECT_DIR="$D/hello-backend" JET_BACKEND_PROJECT_PACKAGE="$HELLO_PKG" \
    JET_BACKEND_PROJECT_SOURCE="$D/hello-backend/source.rs" "$HOST" --ignored --exact bootstrap_tests::bootstrap_backend_project_from_env \
  < /dev/null > "$D/hello-project.log" 2>&1 || fail hello "backend project hook failed: $D/hello-project.log"
backend "$D/hello-backend/Cargo.toml" "$HELLO_PKG" "$D/hello-build.log" ||
  fail hello "$(grep -m5 -E '^error' "$D/hello-build.log" | cut -c1-200 | tr '\n' ' ') (log $D/hello-build.log)"
out=$("$T/debug/$HELLO_PKG" 2> "$D/hello.stderr")
[ "$out" = hello ] || fail hello "fixture printed '$out' (stderr $D/hello.stderr)"
phase hello "printed hello"
report "ok ($what)"
