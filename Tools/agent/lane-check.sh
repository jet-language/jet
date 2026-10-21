#!/usr/bin/env bash
# lane-check.sh — the ONE command an implementation worker may run.
#
# Why this exists: workers are source-only so that one agent owns the build
# lane and the disk. That rule was costing more than it saved — nine separate
# integration breaks in one session (unclosed `impl`, stale module paths after a
# split, wrong visibility, borrow errors, a renamed enum variant, a mismatched
# delimiter). Every one of them is what `cargo check` prints in seconds.
#
# So a worker now type-checks its own patch, in the main checkout, against the
# warm shared target dir. Cargo serializes concurrent invocations on its own
# build lock, so explicitly approved disjoint workers may call this at once;
# they queue, they do not corrupt the shared target.
#
# What this is NOT: it does not run tests, does not run formatters, does not
# regenerate artifacts, does not build a release binary. Those stay with the
# orchestrator.
#
# Usage:
#   Tools/agent/lane-check.sh                 # whole workspace, all targets
#   Tools/agent/lane-check.sh -p jet-sema     # one crate while iterating
#   Tools/agent/lane-check.sh --runtime-generators
#     Runtime-only freshness gate (no Cargo); invoke through cheap.sh.
#
# Reading the output: errors are grouped by file. An error in a file YOU did not
# touch belongs to another worker editing the same checkout — report it in your
# final message, do not fix it.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1

if [ "${1:-}" = "--runtime-generators" ]; then
  if [ "$#" -ne 1 ]; then
    echo "usage: lane-check.sh --runtime-generators" >&2
    exit 64
  fi
  Tools/agent/jet-env jet run Tools/agent/RuntimeGenerators -- --check
  code=$?
  if [ "$code" -eq 0 ]; then echo "CHECK OK"
  else echo "CHECK FAILED — runtime generators exited $code"
  fi
  exit "$code"
fi

# The default run also checks the feature-gated bootstrap host
# (Compiler/Bootstrap/Host), so drift against the Jet compiler fails here (#3874).
if [ "$#" -eq 0 ]; then
  set -- --workspace --all-targets --features jet/compiler-bootstrap-host
fi

# Independent Cargo targets must all report their errors in one check.
keep_going=0
for arg in "$@"; do
  if [ "$arg" = --keep-going ]; then keep_going=1; break; fi
done
if [ "$keep_going" -eq 0 ]; then set -- --keep-going "$@"; fi

# /tmp is RAM-backed here, and a check can fork rustc: keep scratch on disk and
# skip incremental artifacts, which reached 24G in one target dir.
scratch="${JET_TEST_SCRATCH:-$HOME/.cache/jet-dev/scratch}"
mkdir -p "$scratch"
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch"
export CARGO_INCREMENTAL=0
export JET_NIX_TMP_CLEANED=1
# One rustc per hardware thread put this machine into swap on a cold build.
# Four keeps a single holder near ~8G peak while staying fast on a warm target
# dir, while the slot gate below bounds shared resource pressure.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

# A memory floor alone is not enough: concurrent callers can read "plenty free"
# in the same instant and then all fork together. The floor has to be paired
# with a hard limit on how many checks may run at once. `flock` gives us that: at most
# JET_CHECK_SLOTS lanes hold a slot, the rest block until one frees.
floor_mb="${JET_MIN_FREE_MB:-8000}"
slots="${JET_CHECK_SLOTS:-3}"
lockdir="${TMPDIR}/jet-check-slots"
mkdir -p "$lockdir"

run_check() {
  for _ in $(seq 1 90); do
    avail="$(awk '/MemAvailable/ {print int($2/1024)}' /proc/meminfo)"
    [ "${avail:-0}" -ge "$floor_mb" ] && break
    echo "lane-check: waiting for memory (${avail}MB < ${floor_mb}MB floor)" >&2
    sleep 10
  done
  timeout 1800 Tools/agent/jet-env cargo check "$@" >"$tmp" 2>&1
}

tmp="$(mktemp "$scratch/jet-lane-check.XXXXXX")"
# Keep the complete stream even when the caller interrupts the check. A
# missing .rc after SIGKILL means incomplete evidence, never success.
trap 'status=$?; printf "%s\n" "$status" > "$tmp.rc"; printf "lane-check: complete Cargo log: %s\n" "$tmp" >&2' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

# Take any free slot; if every slot is busy, block on slot 1 rather than spin.
code=0
got_slot=0
for slot in $(seq 1 "$slots"); do
  exec 9>"$lockdir/$slot"
  if flock -n 9; then
    got_slot=1
    run_check "$@"
    code=$?
    flock -u 9
    break
  fi
  exec 9>&-
done
if [ "$got_slot" -eq 0 ]; then
  exec 9>"$lockdir/1"
  flock 9
  run_check "$@"
  code=$?
  flock -u 9
fi

node - "$tmp" "$code" <<'NODE'
const fs = require("fs");
const text = fs.readFileSync(process.argv[2], "utf8");
const code = Number(process.argv[3]);
const lines = text.split("\n");
const errors = [];
for (let i = 0; i < lines.length; i += 1) {
  if (!/^error(\[|:)/.test(lines[i])) continue;
  const head = lines[i];
  let where = "";
  for (let j = i + 1; j < Math.min(i + 6, lines.length); j += 1) {
    if (/^error(\[|:)/.test(lines[j])) break;
    const m = lines[j].match(/^\s*-->\s+(\S+)/);
    if (m) { where = m[1]; break; }
  }
  errors.push({ head, where });
}
if (errors.length === 0 && code === 0) {
  console.log("CHECK OK");
  process.exit(0);
}
if (errors.length === 0) {
  console.log(`CHECK FAILED — cargo check exited ${code} without a compiler diagnostic`);
  process.stdout.write(text);
  process.exit(0);
}
const byFile = new Map();
for (const e of errors) {
  const key = e.where.replace(/:\d+:\d+$/, "") || "(no location)";
  if (!byFile.has(key)) byFile.set(key, []);
  byFile.get(key).push(e);
}
console.log(`CHECK FAILED — ${errors.length} error(s) in ${byFile.size} file(s)`);
for (const [file, entries] of byFile) {
  console.log(`\n${file}`);
  for (const e of entries) console.log(`  ${e.where ? `${e.where}: ` : ""}${e.head}`);
}
console.log("\n--- complete compiler output ---");
process.stdout.write(text);
NODE
report_code=$?
if [ "$code" -eq 0 ] && [ "$report_code" -ne 0 ]; then code="$report_code"; fi

exit "$code"
