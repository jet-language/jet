#!/usr/bin/env bash
# lane-check.sh — the ONE command an implementation worker may run.
#
# Why this exists: workers are source-only so that one agent owns the build
# lane and the disk. That rule was costing more than it saved — nine separate
# integration breaks in one session (unclosed `impl`, stale module paths after a
# split, wrong visibility, a renamed enum variant, a mismatched delimiter).
# Every one of them is what `jet check` prints in one keep-going pass.
#
# So a worker type-checks its own patch with the admitted native compiler named
# by $JET. The default run checks the whole compiler project (Compiler/, the
# unit the native self-host builds) with JET_KEEP_GOING=1, so every error in
# every item is reported at once. A path argument checks that file or project
# instead.
#
# What this is NOT: it does not run tests, does not run formatters, does not
# regenerate artifacts, does not build a compiler. Those stay with the
# orchestrator.
#
# Usage:
#   JET=/path/to/jet Tools/agent/lane-check.sh            # the whole compiler
#   JET=/path/to/jet Tools/agent/lane-check.sh <path>     # one file or project
#   Tools/agent/lane-check.sh --runtime-generators
#     Runtime-only freshness gate (no compiler); invoke through cheap.sh.
#
# Reading the output: errors are grouped by file. An error in a file YOU did not
# touch belongs to another worker editing the same checkout — report it in your
# final message, do not fix it.
set -uo pipefail

cd "$(dirname "$0")/../.." || exit 1
repo="$PWD"

if [ "${1:-}" = "--runtime-generators" ]; then
  if [ "$#" -ne 1 ]; then
    echo "usage: lane-check.sh --runtime-generators" >&2
    exit 64
  fi
  code=0
  for gen in trap-messages lifecycle stop-exports host-errors observe-messages scheduler-messages services-messages test-messages; do
    Tools/agent/jet-env node "Tools/agent/gen-runtime-$gen.mjs" --check || code=1
  done
  if [ "$code" -eq 0 ]; then echo "CHECK OK"
  else echo "CHECK FAILED — a runtime generator reported stale output"
  fi
  exit "$code"
fi

if [ "$#" -gt 1 ]; then
  echo "usage: JET=/path/to/jet lane-check.sh [path]" >&2
  exit 64
fi
if [ -z "${JET:-}" ] || [ ! -x "$JET" ]; then
  echo "lane-check: JET must name the admitted native jet compiler binary" >&2
  exit 64
fi

# /tmp is RAM-backed here: keep scratch, the assembled unit and logs on disk.
scratch="${JET_TEST_SCRATCH:-$HOME/.cache/jet-dev/scratch}"
mkdir -p "$scratch"
export TMPDIR="$scratch" TMP="$scratch" TEMP="$scratch"
export JET_NIX_TMP_CLEANED=1
tmp="$(mktemp "$scratch/jet-lane-check.XXXXXX")"

if [ "$#" -eq 1 ]; then
  target="$(realpath -- "$1")"
else
  target="$repo/Compiler"
fi
check_dir="$repo"

# A memory floor alone is not enough: concurrent callers can read "plenty free"
# in the same instant and then all start together. The floor has to be paired
# with a hard limit on how many checks may run at once. `flock` gives us that: at
# most JET_CHECK_SLOTS lanes hold a slot, the rest block until one frees. Each
# check runs in its own memory-capped user scope (JET_CHECK_MEM, default 20G:
# the whole-compiler check measured 15.4G). A host without a systemd user
# session (hosted CI) sets JET_CHECK_MEM=off and relies on its own VM limit.
floor_mb="${JET_MIN_FREE_MB:-8000}"
slots="${JET_CHECK_SLOTS:-2}"
lockdir="${TMPDIR}/jet-check-slots"
mkdir -p "$lockdir"

run_check() {
  for _ in $(seq 1 90); do
    avail="$(awk '/MemAvailable/ {print int($2/1024)}' /proc/meminfo)"
    [ "${avail:-0}" -ge "$floor_mb" ] && break
    echo "lane-check: waiting for memory (${avail}MB < ${floor_mb}MB floor)" >&2
    sleep 10
  done
  local scope=(systemd-run --user --scope -q -p MemoryMax="${JET_CHECK_MEM:-20G}" -p MemorySwapMax=0)
  [ "${JET_CHECK_MEM:-}" = off ] && scope=()
  (
    cd "$check_dir" || exit 1
    JET_KEEP_GOING=1 timeout 1800 "${scope[@]}" "$repo/Tools/agent/jet-env" jet check "$target"
  ) >"$tmp" 2>&1
}

# Keep the complete stream even when the caller interrupts the check. A
# missing .rc after SIGKILL means incomplete evidence, never success.
trap 'status=$?; printf "%s\n" "$status" > "$tmp.rc"; printf "lane-check: complete compiler log: %s\n" "$tmp" >&2' EXIT
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
    run_check
    code=$?
    flock -u 9
    break
  fi
  exec 9>&-
done
if [ "$got_slot" -eq 0 ]; then
  exec 9>"$lockdir/1"
  flock 9
  run_check
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
  console.log(`CHECK FAILED — jet check exited ${code} without a compiler diagnostic`);
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
